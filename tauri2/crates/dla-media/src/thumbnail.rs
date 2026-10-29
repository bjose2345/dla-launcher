use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use image::{ImageError, ImageReader, codecs::jpeg::JpegEncoder};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// The longest side of a thumbnail, in pixels.
pub const THUMBNAIL_EDGE: u32 = 480;
/// How much disk the thumbnails may take before the least recently shown go.
pub const THUMBNAIL_CACHE_LIMIT: u64 = 512 * 1024 * 1024;
// A write still unfinished after this long was cut short and is only a leftover.
const STALE_TEMPORARY_FILE: Duration = Duration::from_secs(60 * 60);
const THUMBNAIL_QUALITY: u8 = 80;
// Changing how thumbnails are drawn changes this, so older files are not reused.
const CACHE_FORMAT: &str = "jpeg-v1";

static TEMPORARY_FILES: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Error)]
pub enum ThumbnailError {
    #[error("thumbnail file access failed: {0}")]
    Io(#[from] io::Error),
    #[error("thumbnail image could not be drawn: {0}")]
    Image(#[from] ImageError),
}

/// Small JPEG copies of images, drawn once and kept on disk, so a gallery
/// never has to decode a full-size image for a tile.
#[derive(Clone)]
pub struct ThumbnailCache {
    directory: PathBuf,
}

impl ThumbnailCache {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    pub fn thumbnail(&self, source: &Path) -> Result<Vec<u8>, ThumbnailError> {
        let cached = self.directory.join(format!("{}.jpg", cache_key(source)?));
        match fs::read(&cached) {
            Ok(bytes) => {
                mark_shown(&cached);
                return Ok(bytes);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let bytes = draw(source)?;
        if let Err(error) = self.store(&cached, &bytes) {
            log::warn!(
                "media thumbnail cache write failed for {}: {error}",
                cached.display()
            );
        }
        Ok(bytes)
    }

    /// Once the cache holds more than `limit` bytes, drops the thumbnails
    /// shown longest ago until it is down to three quarters of it, and clears
    /// what interrupted writes left behind. Returns the bytes freed.
    pub fn prune(&self, limit: u64) -> io::Result<u64> {
        let entries = match fs::read_dir(&self.directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error),
        };
        let now = SystemTime::now();
        let mut thumbnails = Vec::new();
        let mut total = 0_u64;
        let mut freed = 0_u64;
        for entry in entries {
            let entry = entry?;
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if !metadata.is_file() {
                continue;
            }
            let path = entry.path();
            let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
            match path.extension().and_then(|value| value.to_str()) {
                Some("tmp") => {
                    let stale = now
                        .duration_since(modified)
                        .is_ok_and(|age| age >= STALE_TEMPORARY_FILE);
                    if stale && fs::remove_file(&path).is_ok() {
                        freed += metadata.len();
                    }
                }
                Some("jpg") => {
                    total += metadata.len();
                    thumbnails.push((modified, metadata.len(), path));
                }
                _ => {}
            }
        }
        if total <= limit {
            return Ok(freed);
        }
        let target = limit / 4 * 3;
        thumbnails.sort_by_key(|(modified, ..)| *modified);
        for (_, length, path) in thumbnails {
            if total <= target {
                break;
            }
            if fs::remove_file(&path).is_ok() {
                total -= length;
                freed += length;
            }
        }
        Ok(freed)
    }

    fn store(&self, cached: &Path, bytes: &[u8]) -> io::Result<()> {
        fs::create_dir_all(&self.directory)?;
        let temporary = cached.with_extension(format!(
            "{}-{}.tmp",
            std::process::id(),
            TEMPORARY_FILES.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&temporary, bytes)?;
        fs::rename(&temporary, cached).inspect_err(|_| {
            let _ = fs::remove_file(&temporary);
        })
    }
}

fn cache_key(source: &Path) -> io::Result<String> {
    let metadata = fs::metadata(source)?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos());
    let absolute = std::path::absolute(source)?;
    let mut hasher = Sha256::new();
    hasher.update(CACHE_FORMAT.as_bytes());
    hasher.update(THUMBNAIL_EDGE.to_le_bytes());
    hasher.update(absolute.as_os_str().as_encoded_bytes());
    hasher.update([0]);
    hasher.update(metadata.len().to_le_bytes());
    hasher.update(modified.to_le_bytes());
    Ok(hex::encode(hasher.finalize()))
}

// The modified time doubles as the time a thumbnail was last shown, which is
// what pruning goes by; a failure only makes it look older.
fn mark_shown(cached: &Path) {
    let _ = fs::File::options()
        .write(true)
        .open(cached)
        .and_then(|file| file.set_modified(SystemTime::now()));
}

fn draw(source: &Path) -> Result<Vec<u8>, ThumbnailError> {
    let image = ImageReader::open(source)?.with_guessed_format()?.decode()?;
    let image = if image.width() > THUMBNAIL_EDGE || image.height() > THUMBNAIL_EDGE {
        image.thumbnail(THUMBNAIL_EDGE, THUMBNAIL_EDGE)
    } else {
        image
    };
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, THUMBNAIL_QUALITY).encode_image(&image.to_rgb8())?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use image::{ImageFormat, RgbImage};

    use super::*;

    #[test]
    fn draws_a_small_jpeg_once_and_then_serves_it_from_disk() {
        let folder = tempfile::tempdir().expect("temporary folder");
        let source = folder.path().join("cg_001.png");
        RgbImage::from_fn(1920, 1080, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 90])
        })
        .save_with_format(&source, ImageFormat::Png)
        .expect("source image");
        let cache = ThumbnailCache::new(folder.path().join("media-thumbnails"));

        let first = cache.thumbnail(&source).expect("thumbnail");
        let drawn = image::load_from_memory_with_format(&first, ImageFormat::Jpeg)
            .expect("a JPEG thumbnail");
        assert_eq!((drawn.width(), drawn.height()), (480, 270));

        let stored = fs::read_dir(folder.path().join("media-thumbnails"))
            .expect("cache folder")
            .map(|entry| entry.expect("cache entry").path())
            .collect::<Vec<_>>();
        assert_eq!(stored.len(), 1);
        assert_eq!(
            stored[0].extension().and_then(|value| value.to_str()),
            Some("jpg")
        );

        fs::write(&stored[0], b"cached").expect("mark the cached file");
        assert_eq!(
            cache.thumbnail(&source).expect("cached thumbnail"),
            b"cached"
        );
    }

    #[test]
    fn keeps_a_small_image_at_its_own_size() {
        let folder = tempfile::tempdir().expect("temporary folder");
        let source = folder.path().join("icon.png");
        RgbImage::new(200, 300)
            .save_with_format(&source, ImageFormat::Png)
            .expect("source image");

        let bytes = ThumbnailCache::new(folder.path().join("cache"))
            .thumbnail(&source)
            .expect("thumbnail");

        let drawn = image::load_from_memory(&bytes).expect("thumbnail image");
        assert_eq!((drawn.width(), drawn.height()), (200, 300));
    }

    fn cache_file(folder: &Path, name: &str, length: usize, age_seconds: u64) -> PathBuf {
        let path = folder.join(name);
        fs::write(&path, vec![0; length]).expect("cache file");
        fs::File::options()
            .write(true)
            .open(&path)
            .and_then(|file| {
                file.set_modified(SystemTime::now() - Duration::from_secs(age_seconds))
            })
            .expect("file age");
        path
    }

    #[test]
    fn drops_the_thumbnails_shown_longest_ago_once_over_the_limit() {
        let folder = tempfile::tempdir().expect("temporary folder");
        let oldest = cache_file(folder.path(), "a.jpg", 100, 400);
        let older = cache_file(folder.path(), "b.jpg", 100, 300);
        let newer = cache_file(folder.path(), "c.jpg", 100, 200);
        let newest = cache_file(folder.path(), "d.jpg", 100, 100);

        let freed = ThumbnailCache::new(folder.path())
            .prune(300)
            .expect("prune");

        assert_eq!(freed, 200);
        assert!(!oldest.exists());
        assert!(!older.exists());
        assert!(newer.exists());
        assert!(newest.exists());
    }

    #[test]
    fn leaves_a_cache_within_its_limit_alone_but_clears_stale_partial_writes() {
        let folder = tempfile::tempdir().expect("temporary folder");
        let kept = cache_file(folder.path(), "a.jpg", 100, 10_000);
        let stale = cache_file(folder.path(), "b.jpg.1-0.tmp", 50, 2 * 60 * 60);
        let writing = cache_file(folder.path(), "c.jpg.1-1.tmp", 50, 10);

        let freed = ThumbnailCache::new(folder.path())
            .prune(1_000)
            .expect("prune");

        assert_eq!(freed, 50);
        assert!(kept.exists());
        assert!(!stale.exists());
        assert!(writing.exists());
        assert_eq!(
            ThumbnailCache::new(folder.path().join("missing"))
                .prune(1_000)
                .expect("missing folder"),
            0
        );
    }

    #[test]
    fn showing_a_cached_thumbnail_keeps_it_from_being_dropped() {
        let folder = tempfile::tempdir().expect("temporary folder");
        let source = folder.path().join("cg_001.png");
        RgbImage::new(64, 64)
            .save_with_format(&source, ImageFormat::Png)
            .expect("source image");
        let cache = ThumbnailCache::new(folder.path().join("cache"));
        cache.thumbnail(&source).expect("thumbnail");
        let cached = fs::read_dir(folder.path().join("cache"))
            .expect("cache folder")
            .next()
            .expect("cached thumbnail")
            .expect("cache entry")
            .path();
        let day_ago = SystemTime::now() - Duration::from_secs(24 * 60 * 60);
        fs::File::options()
            .write(true)
            .open(&cached)
            .and_then(|file| file.set_modified(day_ago))
            .expect("age the thumbnail");

        cache.thumbnail(&source).expect("cached thumbnail");

        let shown = fs::metadata(&cached)
            .and_then(|metadata| metadata.modified())
            .expect("modified time");
        assert!(shown > day_ago + Duration::from_secs(60));
    }

    #[test]
    fn reports_a_file_that_is_not_an_image() {
        let folder = tempfile::tempdir().expect("temporary folder");
        let source = folder.path().join("broken.png");
        fs::write(&source, b"not an image").expect("source file");

        let result = ThumbnailCache::new(folder.path().join("cache")).thumbnail(&source);

        assert!(matches!(result, Err(ThumbnailError::Image(_))));
    }
}
