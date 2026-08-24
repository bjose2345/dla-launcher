use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use dla_application::catalog_artwork::{
    CatalogArtworkAsset, CatalogArtworkCache, CatalogArtworkCacheStatus,
    CatalogArtworkCacheSummary, CatalogArtworkCapacity, CatalogArtworkError,
    CatalogArtworkMediaType, CatalogArtworkRetention,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ureq::{Agent, http::StatusCode};
use url::Url;
use uuid::Uuid;

const CACHE_FORMAT_VERSION: u8 = 1;
const STRIPE_COUNT: usize = 64;
const DEFAULT_MAX_ASSET_BYTES: u64 = 16 * 1024 * 1024;
const DEFAULT_MAX_CACHE_BYTES: u64 = 512 * 1024 * 1024;
const DEFAULT_MAX_ENTRIES: usize = 4_000;
const DEFAULT_REFRESH_AFTER: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_REDIRECTS: usize = 3;
const MAX_SOURCE_LENGTH: usize = 8_192;

#[derive(Clone, Debug)]
pub struct CoverCachePolicy {
    pub allowed_hosts: Vec<String>,
    pub max_asset_bytes: u64,
    pub max_cache_bytes: u64,
    pub max_entries: usize,
    pub refresh_after: Duration,
}

impl Default for CoverCachePolicy {
    fn default() -> Self {
        Self {
            allowed_hosts: vec![
                "img.dlsite.jp".to_owned(),
                "img.dlsitearchive.com".to_owned(),
            ],
            max_asset_bytes: DEFAULT_MAX_ASSET_BYTES,
            max_cache_bytes: DEFAULT_MAX_CACHE_BYTES,
            max_entries: DEFAULT_MAX_ENTRIES,
            refresh_after: DEFAULT_REFRESH_AFTER,
        }
    }
}

pub struct DesktopCatalogArtworkCache {
    root: PathBuf,
    preference_path: PathBuf,
    fetcher: Arc<dyn RemoteArtworkFetcher>,
    policy: CoverCachePolicy,
    retention: Mutex<CatalogArtworkRetention>,
    capacity: Mutex<CatalogArtworkCapacity>,
    stripes: [Mutex<()>; STRIPE_COUNT],
    commit_lock: Mutex<()>,
    usage: Mutex<CacheUsage>,
}

impl DesktopCatalogArtworkCache {
    pub fn open(
        root: impl Into<PathBuf>,
        preference_path: impl Into<PathBuf>,
    ) -> Result<Self, CatalogArtworkError> {
        let root = root.into();
        let preference_path = preference_path.into();
        let preferences = read_cache_preferences(&preference_path)?;
        Self::open_with(
            root,
            preference_path,
            CoverCachePolicy::default(),
            preferences,
            Arc::new(UreqArtworkFetcher::new()),
        )
    }

    fn open_with(
        root: PathBuf,
        preference_path: PathBuf,
        policy: CoverCachePolicy,
        preferences: CachePreferences,
        fetcher: Arc<dyn RemoteArtworkFetcher>,
    ) -> Result<Self, CatalogArtworkError> {
        prepare_cache_root(&root)?;
        let usage = reconcile_cache(
            &root,
            &policy,
            preferences.retention,
            preferences.capacity,
            true,
            None,
        )?;
        Ok(Self {
            root,
            preference_path,
            fetcher,
            retention: Mutex::new(preferences.retention),
            capacity: Mutex::new(preferences.capacity),
            policy,
            stripes: std::array::from_fn(|_| Mutex::new(())),
            commit_lock: Mutex::new(()),
            usage: Mutex::new(usage),
        })
    }

    fn load_validated(&self, source_url: &Url) -> Result<CatalogArtworkAsset, CatalogArtworkError> {
        let key = cache_key(source_url.as_str());
        let stripe = stripe_for_key(&key);
        let _stripe_guard = self.stripes[stripe]
            .lock()
            .map_err(|_| storage_error("cache request lock is poisoned"))?;
        let now = unix_timestamp()?;
        let cached = self.read_entry(&key)?;

        if let Some(entry) = cached.as_ref()
            && entry.is_fresh(now, self.policy.refresh_after)
        {
            return Ok(entry.asset(CatalogArtworkCacheStatus::Hit));
        }

        let validators = cached.as_ref().map(|entry| RemoteValidators {
            etag: entry.metadata.etag.clone(),
            last_modified: entry.metadata.last_modified.clone(),
        });
        match self
            .fetcher
            .fetch(source_url, validators.as_ref(), &self.policy)
        {
            Ok(RemoteArtworkResponse::NotModified) => {
                let Some(mut entry) = cached else {
                    return Err(CatalogArtworkError::SourceUnavailable(
                        "the source returned an invalid cache response".to_owned(),
                    ));
                };
                entry.metadata.fetched_at = now;
                self.write_metadata(&key, &entry.metadata)?;
                Ok(entry.asset(CatalogArtworkCacheStatus::Revalidated))
            }
            Ok(RemoteArtworkResponse::Downloaded(download)) => {
                if download.bytes.len() as u64 > self.policy.max_asset_bytes {
                    return cached.map_or(Err(CatalogArtworkError::TooLarge), |entry| {
                        Ok(entry.asset(CatalogArtworkCacheStatus::Stale))
                    });
                }
                let Some(media_type) = detect_media_type(&download.bytes) else {
                    return cached.map_or(Err(CatalogArtworkError::UnsupportedImage), |entry| {
                        Ok(entry.asset(CatalogArtworkCacheStatus::Stale))
                    });
                };
                let metadata = CacheMetadata {
                    version: CACHE_FORMAT_VERSION,
                    media_type,
                    size: download.bytes.len() as u64,
                    fetched_at: now,
                    etag: download.etag,
                    last_modified: download.last_modified,
                };
                self.write_entry(&key, &download.bytes, &metadata)?;
                Ok(CatalogArtworkAsset {
                    bytes: download.bytes,
                    media_type,
                    cache_status: CatalogArtworkCacheStatus::Miss,
                })
            }
            Err(error) => cached.map_or(Err(error), |entry| {
                Ok(entry.asset(CatalogArtworkCacheStatus::Stale))
            }),
        }
    }

    fn read_entry(&self, key: &str) -> Result<Option<CachedEntry>, CatalogArtworkError> {
        let paths = CachePaths::new(&self.root, key);
        let metadata = match read_regular_file(&paths.metadata, 64 * 1024)? {
            Some(bytes) => match serde_json::from_slice::<CacheMetadata>(&bytes) {
                Ok(metadata) if metadata.is_valid(&self.policy) => metadata,
                _ => {
                    self.remove_entry(&paths)?;
                    return Ok(None);
                }
            },
            None => {
                if is_regular_file(&paths.body)? {
                    self.remove_entry(&paths)?;
                }
                return Ok(None);
            }
        };
        let Some(bytes) = read_regular_file(&paths.body, self.policy.max_asset_bytes)? else {
            self.remove_entry(&paths)?;
            return Ok(None);
        };
        if bytes.len() as u64 != metadata.size
            || detect_media_type(&bytes) != Some(metadata.media_type)
        {
            self.remove_entry(&paths)?;
            return Ok(None);
        }
        Ok(Some(CachedEntry { bytes, metadata }))
    }

    fn write_entry(
        &self,
        key: &str,
        bytes: &[u8],
        metadata: &CacheMetadata,
    ) -> Result<(), CatalogArtworkError> {
        let _commit_guard = self
            .commit_lock
            .lock()
            .map_err(|_| storage_error("cache commit lock is poisoned"))?;
        let paths = CachePaths::new(&self.root, key);
        let previous_size = regular_file_length(&paths.body)?.unwrap_or(0);
        let previous_entry = is_regular_file(&paths.metadata)? && previous_size > 0;
        atomic_write(&paths.body, bytes)?;
        let metadata_bytes = serde_json::to_vec(metadata).map_err(storage_error)?;
        if let Err(error) = atomic_write(&paths.metadata, &metadata_bytes) {
            let _ = remove_regular_file(&paths.body);
            return Err(error);
        }

        let retention = *self
            .retention
            .lock()
            .map_err(|_| storage_error("cache retention lock is poisoned"))?;
        let capacity = *self
            .capacity
            .lock()
            .map_err(|_| storage_error("cache capacity lock is poisoned"))?;
        let mut usage = self
            .usage
            .lock()
            .map_err(|_| storage_error("cache usage lock is poisoned"))?;
        usage.bytes = usage
            .bytes
            .saturating_sub(previous_size)
            .saturating_add(bytes.len() as u64);
        if !previous_entry {
            usage.entries = usage.entries.saturating_add(1);
        }
        if cache_limits_exceeded(&self.policy, capacity, &usage) {
            *usage = reconcile_cache(
                &self.root,
                &self.policy,
                retention,
                capacity,
                false,
                Some(key),
            )?;
        }
        Ok(())
    }

    fn write_metadata(
        &self,
        key: &str,
        metadata: &CacheMetadata,
    ) -> Result<(), CatalogArtworkError> {
        let _commit_guard = self
            .commit_lock
            .lock()
            .map_err(|_| storage_error("cache commit lock is poisoned"))?;
        let bytes = serde_json::to_vec(metadata).map_err(storage_error)?;
        atomic_write(&CachePaths::new(&self.root, key).metadata, &bytes)
    }

    fn remove_entry(&self, paths: &CachePaths) -> Result<(), CatalogArtworkError> {
        let _commit_guard = self
            .commit_lock
            .lock()
            .map_err(|_| storage_error("cache commit lock is poisoned"))?;
        let removed_size = regular_file_length(&paths.body)?.unwrap_or(0);
        let existed = is_regular_file(&paths.metadata)? || removed_size > 0;
        remove_regular_file(&paths.body)?;
        remove_regular_file(&paths.metadata)?;
        if existed {
            let mut usage = self
                .usage
                .lock()
                .map_err(|_| storage_error("cache usage lock is poisoned"))?;
            usage.bytes = usage.bytes.saturating_sub(removed_size);
            usage.entries = usage.entries.saturating_sub(1);
        }
        Ok(())
    }
}

impl CatalogArtworkCache for DesktopCatalogArtworkCache {
    fn load(&self, source_url: &str) -> Result<CatalogArtworkAsset, CatalogArtworkError> {
        let source_url = validate_source(source_url, &self.policy.allowed_hosts)?;
        self.load_validated(&source_url)
    }

    fn summary(&self) -> Result<CatalogArtworkCacheSummary, CatalogArtworkError> {
        let retention = *self
            .retention
            .lock()
            .map_err(|_| storage_error("cache retention lock is poisoned"))?;
        let capacity = *self
            .capacity
            .lock()
            .map_err(|_| storage_error("cache capacity lock is poisoned"))?;
        let usage = self
            .usage
            .lock()
            .map_err(|_| storage_error("cache usage lock is poisoned"))?;
        let limits = cache_limits(&self.policy, capacity);
        Ok(CatalogArtworkCacheSummary {
            retention,
            capacity,
            entry_count: usage.entries,
            stored_bytes: usage.bytes,
            maximum_bytes: limits.map(|(bytes, _)| bytes),
            maximum_entries: limits.map(|(_, entries)| entries),
        })
    }

    fn configure(
        &self,
        retention: CatalogArtworkRetention,
        capacity: CatalogArtworkCapacity,
    ) -> Result<CatalogArtworkCacheSummary, CatalogArtworkError> {
        let _commit_guard = self
            .commit_lock
            .lock()
            .map_err(|_| storage_error("cache commit lock is poisoned"))?;
        write_cache_preferences(
            &self.preference_path,
            CachePreferences {
                retention,
                capacity,
            },
        )?;
        let usage = reconcile_cache(&self.root, &self.policy, retention, capacity, false, None)?;
        let limits = cache_limits(&self.policy, capacity);
        let summary = CatalogArtworkCacheSummary {
            retention,
            capacity,
            entry_count: usage.entries,
            stored_bytes: usage.bytes,
            maximum_bytes: limits.map(|(bytes, _)| bytes),
            maximum_entries: limits.map(|(_, entries)| entries),
        };
        *self
            .retention
            .lock()
            .map_err(|_| storage_error("cache retention lock is poisoned"))? = retention;
        *self
            .capacity
            .lock()
            .map_err(|_| storage_error("cache capacity lock is poisoned"))? = capacity;
        *self
            .usage
            .lock()
            .map_err(|_| storage_error("cache usage lock is poisoned"))? = usage;
        Ok(summary)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct CacheMetadata {
    version: u8,
    media_type: CatalogArtworkMediaType,
    size: u64,
    fetched_at: u64,
    etag: Option<String>,
    last_modified: Option<String>,
}

impl CacheMetadata {
    fn is_valid(&self, policy: &CoverCachePolicy) -> bool {
        self.version == CACHE_FORMAT_VERSION && self.size > 0 && self.size <= policy.max_asset_bytes
    }
}

struct CachedEntry {
    bytes: Vec<u8>,
    metadata: CacheMetadata,
}

impl CachedEntry {
    fn is_fresh(&self, now: u64, refresh_after: Duration) -> bool {
        now.saturating_sub(self.metadata.fetched_at) < refresh_after.as_secs()
    }

    fn asset(&self, cache_status: CatalogArtworkCacheStatus) -> CatalogArtworkAsset {
        CatalogArtworkAsset {
            bytes: self.bytes.clone(),
            media_type: self.metadata.media_type,
            cache_status,
        }
    }
}

#[derive(Clone, Default)]
struct RemoteValidators {
    etag: Option<String>,
    last_modified: Option<String>,
}

struct DownloadedArtwork {
    bytes: Vec<u8>,
    etag: Option<String>,
    last_modified: Option<String>,
}

enum RemoteArtworkResponse {
    Downloaded(DownloadedArtwork),
    NotModified,
}

trait RemoteArtworkFetcher: Send + Sync {
    fn fetch(
        &self,
        source_url: &Url,
        validators: Option<&RemoteValidators>,
        policy: &CoverCachePolicy,
    ) -> Result<RemoteArtworkResponse, CatalogArtworkError>;
}

struct UreqArtworkFetcher {
    agent: Agent,
}

impl UreqArtworkFetcher {
    fn new() -> Self {
        let config = Agent::config_builder()
            .https_only(true)
            .http_status_as_error(false)
            .max_redirects(0)
            .timeout_global(Some(REQUEST_TIMEOUT))
            .user_agent("DLA-Launcher/cover-cache")
            .build();
        Self {
            agent: Agent::new_with_config(config),
        }
    }
}

impl RemoteArtworkFetcher for UreqArtworkFetcher {
    fn fetch(
        &self,
        source_url: &Url,
        validators: Option<&RemoteValidators>,
        policy: &CoverCachePolicy,
    ) -> Result<RemoteArtworkResponse, CatalogArtworkError> {
        let mut current = source_url.clone();
        for redirect_count in 0..=MAX_REDIRECTS {
            let mut request = self
                .agent
                .get(current.as_str())
                .header(
                    "accept",
                    "image/avif,image/webp,image/png,image/jpeg,image/gif",
                )
                .header("accept-encoding", "identity");
            if redirect_count == 0
                && let Some(validators) = validators
            {
                if let Some(etag) = validators.etag.as_deref() {
                    request = request.header("if-none-match", etag);
                }
                if let Some(last_modified) = validators.last_modified.as_deref() {
                    request = request.header("if-modified-since", last_modified);
                }
            }
            let mut response = request.call().map_err(|_| {
                CatalogArtworkError::SourceUnavailable("the request failed".to_owned())
            })?;
            let status = response.status();
            if status == StatusCode::NOT_MODIFIED {
                return Ok(RemoteArtworkResponse::NotModified);
            }
            if status.is_redirection() {
                if redirect_count == MAX_REDIRECTS {
                    return Err(CatalogArtworkError::SourceUnavailable(
                        "the source redirected too many times".to_owned(),
                    ));
                }
                let location = response
                    .headers()
                    .get("location")
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(|| {
                        CatalogArtworkError::SourceUnavailable(
                            "the source returned an invalid redirect".to_owned(),
                        )
                    })?;
                let redirected = current.join(location).map_err(|_| {
                    CatalogArtworkError::SourceUnavailable(
                        "the source returned an invalid redirect".to_owned(),
                    )
                })?;
                current = validate_source(redirected.as_str(), &policy.allowed_hosts)?;
                continue;
            }
            if status == StatusCode::NOT_FOUND {
                return Err(CatalogArtworkError::NotFound);
            }
            if !status.is_success() {
                return Err(CatalogArtworkError::SourceUnavailable(format!(
                    "the source returned status {}",
                    status.as_u16()
                )));
            }
            if let Some(length) = response
                .headers()
                .get("content-length")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                && length > policy.max_asset_bytes
            {
                return Err(CatalogArtworkError::TooLarge);
            }
            let etag = response
                .headers()
                .get("etag")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
            let last_modified = response
                .headers()
                .get("last-modified")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
            let mut bytes = Vec::new();
            response
                .body_mut()
                .as_reader()
                .take(policy.max_asset_bytes.saturating_add(1))
                .read_to_end(&mut bytes)
                .map_err(|_| {
                    CatalogArtworkError::SourceUnavailable(
                        "the response body could not be read".to_owned(),
                    )
                })?;
            if bytes.len() as u64 > policy.max_asset_bytes {
                return Err(CatalogArtworkError::TooLarge);
            }
            return Ok(RemoteArtworkResponse::Downloaded(DownloadedArtwork {
                bytes,
                etag,
                last_modified,
            }));
        }
        Err(CatalogArtworkError::SourceUnavailable(
            "the source redirected too many times".to_owned(),
        ))
    }
}

#[derive(Default)]
struct CacheUsage {
    bytes: u64,
    entries: usize,
}

struct CachePaths {
    body: PathBuf,
    metadata: PathBuf,
}

impl CachePaths {
    fn new(root: &Path, key: &str) -> Self {
        Self {
            body: root.join(format!("{key}.image")),
            metadata: root.join(format!("{key}.json")),
        }
    }
}

struct ReconciledEntry {
    key: String,
    paths: CachePaths,
    size: u64,
    fetched_at: u64,
}

fn prepare_cache_root(root: &Path) -> Result<(), CatalogArtworkError> {
    match fs::symlink_metadata(root) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(storage_error("cover cache root is not a regular directory"));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(root).map_err(storage_error)?;
        }
        Err(error) => return Err(storage_error(error)),
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root, fs::Permissions::from_mode(0o700)).map_err(storage_error)?;
    }
    Ok(())
}

fn reconcile_cache(
    root: &Path,
    policy: &CoverCachePolicy,
    retention: CatalogArtworkRetention,
    capacity: CatalogArtworkCapacity,
    remove_temporary: bool,
    protected_key: Option<&str>,
) -> Result<CacheUsage, CatalogArtworkError> {
    let now = unix_timestamp()?;
    let mut metadata_keys = HashSet::new();
    let mut entries = Vec::new();
    for item in fs::read_dir(root).map_err(storage_error)? {
        let item = item.map_err(storage_error)?;
        let path = item.path();
        let name = item.file_name();
        let name = name.to_string_lossy();
        if remove_temporary && name.contains(".tmp-") {
            remove_regular_file(&path)?;
            continue;
        }
        let Some(key) = name
            .strip_suffix(".json")
            .filter(|key| valid_cache_key(key))
        else {
            continue;
        };
        let paths = CachePaths::new(root, key);
        let metadata_bytes = match read_regular_file(&paths.metadata, 64 * 1024)? {
            Some(bytes) => bytes,
            None => {
                remove_regular_file(&paths.metadata)?;
                remove_regular_file(&paths.body)?;
                continue;
            }
        };
        let metadata = match serde_json::from_slice::<CacheMetadata>(&metadata_bytes) {
            Ok(metadata) if metadata.is_valid(policy) => metadata,
            _ => {
                remove_regular_file(&paths.metadata)?;
                remove_regular_file(&paths.body)?;
                continue;
            }
        };
        let Some(size) = regular_file_length(&paths.body)? else {
            remove_regular_file(&paths.metadata)?;
            remove_regular_file(&paths.body)?;
            continue;
        };
        let expired = retention.days().is_some_and(|days| {
            now.saturating_sub(metadata.fetched_at)
                > Duration::from_secs(days * 24 * 60 * 60).as_secs()
        });
        if size != metadata.size || expired {
            remove_regular_file(&paths.metadata)?;
            remove_regular_file(&paths.body)?;
            continue;
        }
        metadata_keys.insert(key.to_owned());
        entries.push(ReconciledEntry {
            key: key.to_owned(),
            paths,
            size,
            fetched_at: metadata.fetched_at,
        });
    }
    for item in fs::read_dir(root).map_err(storage_error)? {
        let item = item.map_err(storage_error)?;
        let name = item.file_name();
        let name = name.to_string_lossy();
        let Some(key) = name
            .strip_suffix(".image")
            .filter(|key| valid_cache_key(key))
        else {
            continue;
        };
        if !metadata_keys.contains(key) {
            remove_regular_file(&item.path())?;
        }
    }
    entries.sort_by_key(|entry| {
        (
            entry.fetched_at,
            protected_key.is_some_and(|key| key == entry.key),
        )
    });
    let mut usage = CacheUsage {
        bytes: entries.iter().map(|entry| entry.size).sum(),
        entries: entries.len(),
    };
    if let Some((maximum_bytes, maximum_entries)) = cache_limits(policy, capacity) {
        for entry in entries {
            if usage.bytes <= maximum_bytes && usage.entries <= maximum_entries {
                break;
            }
            remove_regular_file(&entry.paths.metadata)?;
            remove_regular_file(&entry.paths.body)?;
            usage.bytes = usage.bytes.saturating_sub(entry.size);
            usage.entries = usage.entries.saturating_sub(1);
        }
    }
    Ok(usage)
}

fn cache_limits(
    policy: &CoverCachePolicy,
    capacity: CatalogArtworkCapacity,
) -> Option<(u64, usize)> {
    match capacity {
        CatalogArtworkCapacity::Standard => Some((policy.max_cache_bytes, policy.max_entries)),
        other => other.limits(),
    }
}

fn cache_limits_exceeded(
    policy: &CoverCachePolicy,
    capacity: CatalogArtworkCapacity,
    usage: &CacheUsage,
) -> bool {
    cache_limits(policy, capacity)
        .is_some_and(|(bytes, entries)| usage.bytes > bytes || usage.entries > entries)
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
struct CachePreferences {
    retention: CatalogArtworkRetention,
    capacity: CatalogArtworkCapacity,
}

impl Default for CachePreferences {
    fn default() -> Self {
        Self {
            retention: CatalogArtworkRetention::Days180,
            capacity: CatalogArtworkCapacity::Standard,
        }
    }
}

fn read_cache_preferences(path: &Path) -> Result<CachePreferences, CatalogArtworkError> {
    let Some(bytes) = read_regular_file(path, 64 * 1024)? else {
        return Ok(CachePreferences::default());
    };
    Ok(serde_json::from_slice(&bytes).unwrap_or_default())
}

fn write_cache_preferences(
    path: &Path,
    preferences: CachePreferences,
) -> Result<(), CatalogArtworkError> {
    let parent = path
        .parent()
        .ok_or_else(|| storage_error("cover cache preference has no parent directory"))?;
    fs::create_dir_all(parent).map_err(storage_error)?;
    let bytes = serde_json::to_vec(&preferences).map_err(storage_error)?;
    atomic_write(path, &bytes)
}

fn validate_source(source_url: &str, allowed_hosts: &[String]) -> Result<Url, CatalogArtworkError> {
    if source_url.len() > MAX_SOURCE_LENGTH {
        return Err(CatalogArtworkError::InvalidSource);
    }
    let url = Url::parse(source_url).map_err(|_| CatalogArtworkError::InvalidSource)?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.fragment().is_some()
    {
        return Err(CatalogArtworkError::SourceNotAllowed);
    }
    let host = url
        .host_str()
        .ok_or(CatalogArtworkError::InvalidSource)?
        .to_ascii_lowercase();
    if !allowed_hosts.iter().any(|allowed| allowed == &host) {
        return Err(CatalogArtworkError::SourceNotAllowed);
    }
    Ok(url)
}

fn detect_media_type(bytes: &[u8]) -> Option<CatalogArtworkMediaType> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(CatalogArtworkMediaType::Jpeg);
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(CatalogArtworkMediaType::Png);
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some(CatalogArtworkMediaType::Gif);
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some(CatalogArtworkMediaType::Webp);
    }
    if bytes.len() >= 12
        && &bytes[4..8] == b"ftyp"
        && bytes[8..bytes.len().min(32)]
            .chunks_exact(4)
            .any(|brand| brand == b"avif" || brand == b"avis")
    {
        return Some(CatalogArtworkMediaType::Avif);
    }
    None
}

fn cache_key(source_url: &str) -> String {
    hex::encode(Sha256::digest(source_url.as_bytes()))
}

fn stripe_for_key(key: &str) -> usize {
    usize::from_str_radix(&key[..2], 16).unwrap_or_default() % STRIPE_COUNT
}

fn valid_cache_key(key: &str) -> bool {
    key.len() == 64 && key.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn unix_timestamp() -> Result<u64, CatalogArtworkError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(storage_error)
}

fn read_regular_file(path: &Path, max_bytes: u64) -> Result<Option<Vec<u8>>, CatalogArtworkError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(storage_error(error)),
    };
    if !metadata.file_type().is_file() || metadata.len() > max_bytes {
        return Ok(None);
    }
    let capacity = usize::try_from(metadata.len()).map_err(storage_error)?;
    let mut bytes = Vec::with_capacity(capacity);
    File::open(path)
        .map_err(storage_error)?
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(storage_error)?;
    if bytes.len() as u64 > max_bytes {
        return Ok(None);
    }
    Ok(Some(bytes))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), CatalogArtworkError> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| storage_error("cache entry has no filename"))?;
    let temporary = path.with_file_name(format!("{file_name}.tmp-{}", Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(storage_error)?;
        file.write_all(bytes).map_err(storage_error)?;
        file.sync_all().map_err(storage_error)?;
        fs::rename(&temporary, path).map_err(storage_error)
    })();
    if result.is_err() {
        let _ = remove_regular_file(&temporary);
    }
    result
}

fn is_regular_file(path: &Path) -> Result<bool, CatalogArtworkError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(metadata.file_type().is_file()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(storage_error(error)),
    }
}

fn regular_file_length(path: &Path) -> Result<Option<u64>, CatalogArtworkError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(Some(metadata.len())),
        Ok(_) => Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(storage_error(error)),
    }
}

fn remove_regular_file(path: &Path) -> Result<(), CatalogArtworkError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() || metadata.file_type().is_symlink() => {
            fs::remove_file(path).map_err(storage_error)
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(storage_error(error)),
    }
}

fn storage_error(error: impl std::fmt::Display) -> CatalogArtworkError {
    CatalogArtworkError::Storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use tempfile::tempdir;

    use super::*;

    const SOURCE: &str =
        "https://img.dlsite.jp/modpub/images2/work/doujin/RJ000000/RJ000001_img_main.webp";

    struct FakeFetcher {
        calls: AtomicUsize,
        outcomes: Mutex<VecDeque<FakeOutcome>>,
    }

    enum FakeOutcome {
        Download(Vec<u8>),
        NotModified,
        Unavailable,
    }

    impl FakeFetcher {
        fn new(outcomes: impl IntoIterator<Item = FakeOutcome>) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                outcomes: Mutex::new(outcomes.into_iter().collect()),
            }
        }
    }

    impl RemoteArtworkFetcher for FakeFetcher {
        fn fetch(
            &self,
            _source_url: &Url,
            _validators: Option<&RemoteValidators>,
            _policy: &CoverCachePolicy,
        ) -> Result<RemoteArtworkResponse, CatalogArtworkError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match self.outcomes.lock().unwrap().pop_front().unwrap() {
                FakeOutcome::Download(bytes) => {
                    Ok(RemoteArtworkResponse::Downloaded(DownloadedArtwork {
                        bytes,
                        etag: Some("\"fixture\"".to_owned()),
                        last_modified: None,
                    }))
                }
                FakeOutcome::NotModified => Ok(RemoteArtworkResponse::NotModified),
                FakeOutcome::Unavailable => {
                    Err(CatalogArtworkError::SourceUnavailable("offline".to_owned()))
                }
            }
        }
    }

    fn png(seed: u8) -> Vec<u8> {
        [b"\x89PNG\r\n\x1a\n".as_slice(), &[seed; 8]].concat()
    }

    fn cache(
        directory: &Path,
        fetcher: Arc<FakeFetcher>,
        policy: CoverCachePolicy,
    ) -> DesktopCatalogArtworkCache {
        cache_with_preferences(directory, fetcher, policy, CachePreferences::default())
    }

    fn cache_with_preferences(
        directory: &Path,
        fetcher: Arc<FakeFetcher>,
        policy: CoverCachePolicy,
        preferences: CachePreferences,
    ) -> DesktopCatalogArtworkCache {
        DesktopCatalogArtworkCache::open_with(
            directory.to_owned(),
            directory.join("preferences.json"),
            policy,
            preferences,
            fetcher,
        )
        .unwrap()
    }

    #[test]
    fn writes_a_miss_and_serves_the_next_request_from_disk() {
        let directory = tempdir().unwrap();
        let fetcher = Arc::new(FakeFetcher::new([FakeOutcome::Download(png(1))]));
        let cache = cache(
            directory.path(),
            Arc::clone(&fetcher),
            CoverCachePolicy::default(),
        );

        let first = cache.load(SOURCE).unwrap();
        let second = cache.load(SOURCE).unwrap();

        assert_eq!(first.cache_status, CatalogArtworkCacheStatus::Miss);
        assert_eq!(second.cache_status, CatalogArtworkCacheStatus::Hit);
        assert_eq!(first.bytes, second.bytes);
        assert_eq!(fetcher.calls.load(Ordering::SeqCst), 1);
        let persisted = fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(persisted.iter().all(|name| !name.contains("img.dlsite")));
    }

    #[test]
    fn rejects_non_https_and_unapproved_hosts_without_fetching() {
        let directory = tempdir().unwrap();
        let fetcher = Arc::new(FakeFetcher::new([]));
        let cache = cache(
            directory.path(),
            Arc::clone(&fetcher),
            CoverCachePolicy::default(),
        );

        assert!(matches!(
            cache.load("http://img.dlsite.jp/cover.webp"),
            Err(CatalogArtworkError::SourceNotAllowed)
        ));
        assert!(matches!(
            cache.load("https://127.0.0.1/cover.webp"),
            Err(CatalogArtworkError::SourceNotAllowed)
        ));
        assert_eq!(fetcher.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn revalidates_stale_entries_and_keeps_the_cached_body() {
        let directory = tempdir().unwrap();
        let fetcher = Arc::new(FakeFetcher::new([
            FakeOutcome::Download(png(2)),
            FakeOutcome::NotModified,
        ]));
        let policy = CoverCachePolicy {
            refresh_after: Duration::ZERO,
            ..CoverCachePolicy::default()
        };
        let cache = cache(directory.path(), Arc::clone(&fetcher), policy);

        cache.load(SOURCE).unwrap();
        let second = cache.load(SOURCE).unwrap();

        assert_eq!(second.cache_status, CatalogArtworkCacheStatus::Revalidated);
        assert_eq!(second.bytes, png(2));
        assert_eq!(fetcher.calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn serves_stale_artwork_when_refresh_is_offline() {
        let directory = tempdir().unwrap();
        let fetcher = Arc::new(FakeFetcher::new([
            FakeOutcome::Download(png(3)),
            FakeOutcome::Unavailable,
        ]));
        let policy = CoverCachePolicy {
            refresh_after: Duration::ZERO,
            ..CoverCachePolicy::default()
        };
        let cache = cache(directory.path(), fetcher, policy);

        cache.load(SOURCE).unwrap();
        let stale = cache.load(SOURCE).unwrap();

        assert_eq!(stale.cache_status, CatalogArtworkCacheStatus::Stale);
        assert_eq!(stale.bytes, png(3));
    }

    #[test]
    fn rejects_unsupported_and_oversized_downloads() {
        let directory = tempdir().unwrap();
        let fetcher = Arc::new(FakeFetcher::new([
            FakeOutcome::Download(b"bad".to_vec()),
            FakeOutcome::Download(png(4)),
        ]));
        let policy = CoverCachePolicy {
            max_asset_bytes: 12,
            ..CoverCachePolicy::default()
        };
        let cache = cache(directory.path(), fetcher, policy);

        assert!(matches!(
            cache.load(SOURCE),
            Err(CatalogArtworkError::UnsupportedImage)
        ));
        assert!(matches!(
            cache.load(SOURCE),
            Err(CatalogArtworkError::TooLarge)
        ));
    }

    #[test]
    fn evicts_the_oldest_entry_when_the_entry_limit_is_reached() {
        let directory = tempdir().unwrap();
        let source_two = "https://img.dlsitearchive.com/cover-two.webp";
        let fetcher = Arc::new(FakeFetcher::new([
            FakeOutcome::Download(png(5)),
            FakeOutcome::Download(png(6)),
            FakeOutcome::Download(png(7)),
        ]));
        let policy = CoverCachePolicy {
            max_entries: 1,
            ..CoverCachePolicy::default()
        };
        let cache = cache(directory.path(), Arc::clone(&fetcher), policy);

        cache.load(SOURCE).unwrap();
        cache.load(source_two).unwrap();
        cache.load(SOURCE).unwrap();

        assert_eq!(fetcher.calls.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn unlimited_capacity_does_not_apply_the_size_or_entry_ceiling() {
        let directory = tempdir().unwrap();
        let source_two = "https://img.dlsitearchive.com/cover-two.webp";
        let fetcher = Arc::new(FakeFetcher::new([
            FakeOutcome::Download(png(8)),
            FakeOutcome::Download(png(9)),
        ]));
        let policy = CoverCachePolicy {
            max_entries: 1,
            ..CoverCachePolicy::default()
        };
        let cache = cache_with_preferences(
            directory.path(),
            Arc::clone(&fetcher),
            policy,
            CachePreferences {
                retention: CatalogArtworkRetention::Never,
                capacity: CatalogArtworkCapacity::Unlimited,
            },
        );

        cache.load(SOURCE).unwrap();
        cache.load(source_two).unwrap();
        assert_eq!(
            cache.load(SOURCE).unwrap().cache_status,
            CatalogArtworkCacheStatus::Hit
        );
        assert_eq!(cache.summary().unwrap().entry_count, 2);
        assert_eq!(fetcher.calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn persists_retention_and_capacity_outside_the_disposable_cache() {
        let directory = tempdir().unwrap();
        let cache_root = directory.path().join("covers");
        let preference_path = directory.path().join("preferences/cover-cache.json");
        let cache = DesktopCatalogArtworkCache::open(&cache_root, &preference_path).unwrap();

        let summary = cache
            .configure(
                CatalogArtworkRetention::Never,
                CatalogArtworkCapacity::Unlimited,
            )
            .unwrap();
        assert_eq!(summary.maximum_bytes, None);
        drop(cache);

        let reopened = DesktopCatalogArtworkCache::open(&cache_root, &preference_path).unwrap();
        let summary = reopened.summary().unwrap();
        assert_eq!(summary.retention, CatalogArtworkRetention::Never);
        assert_eq!(summary.capacity, CatalogArtworkCapacity::Unlimited);
    }

    #[test]
    fn applying_a_retention_window_removes_expired_entries() {
        let directory = tempdir().unwrap();
        let fetcher = Arc::new(FakeFetcher::new([FakeOutcome::Download(png(10))]));
        let cache = cache(directory.path(), fetcher, CoverCachePolicy::default());
        cache.load(SOURCE).unwrap();
        let paths = CachePaths::new(directory.path(), &cache_key(SOURCE));
        let mut metadata: CacheMetadata =
            serde_json::from_slice(&fs::read(&paths.metadata).unwrap()).unwrap();
        metadata.fetched_at = 0;
        fs::write(&paths.metadata, serde_json::to_vec(&metadata).unwrap()).unwrap();

        let summary = cache
            .configure(
                CatalogArtworkRetention::Days90,
                CatalogArtworkCapacity::Standard,
            )
            .unwrap();

        assert_eq!(summary.entry_count, 0);
        assert!(!paths.body.exists());
        assert!(!paths.metadata.exists());
    }

    #[test]
    fn startup_reconciliation_removes_incomplete_entries() {
        let directory = tempdir().unwrap();
        let key = cache_key(SOURCE);
        let paths = CachePaths::new(directory.path(), &key);
        fs::write(&paths.body, png(11)).unwrap();
        fs::write(directory.path().join("orphan.image.tmp-fixture"), png(12)).unwrap();

        let cache = cache(
            directory.path(),
            Arc::new(FakeFetcher::new([])),
            CoverCachePolicy::default(),
        );

        assert_eq!(cache.summary().unwrap().entry_count, 0);
        assert!(!paths.body.exists());
        assert!(!directory.path().join("orphan.image.tmp-fixture").exists());
    }

    #[test]
    fn detects_only_supported_raster_signatures() {
        assert_eq!(
            detect_media_type(&png(1)),
            Some(CatalogArtworkMediaType::Png)
        );
        assert_eq!(
            detect_media_type(b"RIFF\x08\x00\x00\x00WEBPpayload"),
            Some(CatalogArtworkMediaType::Webp)
        );
        assert_eq!(
            detect_media_type(b"\0\0\0\x18ftypavif\0\0\0\0"),
            Some(CatalogArtworkMediaType::Avif)
        );
        assert_eq!(detect_media_type(b"<svg></svg>"), None);
    }
}
