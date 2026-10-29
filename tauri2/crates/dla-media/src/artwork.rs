use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime},
};

use dla_application::own_work::{OwnWorkArtworkStore, OwnWorkError, is_artwork_id};
use dla_domain::installation::InstallationId;
use image::{DynamicImage, ImageReader, Limits, codecs::jpeg::JpegEncoder, imageops::FilterType};
use sha2::{Digest, Sha256};

/// The longest side of a stored cover or sample, in pixels.
pub const ARTWORK_EDGE: u32 = 1600;
const ARTWORK_QUALITY: u8 = 85;
const MAX_SOURCE_EDGE: u32 = 16_384;
const MAX_DECODE_BYTES: u64 = 512 * 1024 * 1024;
// A picture imported a moment ago may be waiting for the save that uses it.
const UNSAVED_GRACE: Duration = Duration::from_secs(10 * 60);

static TEMPORARY_FILES: AtomicU64 = AtomicU64::new(0);

/// Covers and samples of the user's own works, each a JPEG named by its
/// digest in a folder of its own work.
pub struct OwnWorkArtworkFiles {
    directory: PathBuf,
}

impl OwnWorkArtworkFiles {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    fn folder(&self, installation_id: &InstallationId) -> PathBuf {
        self.directory.join(folder_key(installation_id))
    }

    fn file(
        &self,
        installation_id: &InstallationId,
        artwork_id: &str,
    ) -> Result<PathBuf, OwnWorkError> {
        if !is_artwork_id(artwork_id) {
            return Err(OwnWorkError::UnknownArtwork(artwork_id.to_owned()));
        }
        Ok(self
            .folder(installation_id)
            .join(format!("{artwork_id}.jpg")))
    }
}

impl OwnWorkArtworkStore for OwnWorkArtworkFiles {
    fn import(
        &self,
        installation_id: &InstallationId,
        source: &Path,
    ) -> Result<String, OwnWorkError> {
        let bytes = encode(source)?;
        let artwork_id = hex::encode(Sha256::digest(&bytes));
        let target = self.file(installation_id, &artwork_id)?;
        if !target.is_file() {
            store(&target, &bytes).map_err(OwnWorkError::artwork)?;
        }
        Ok(artwork_id)
    }

    fn contains(
        &self,
        installation_id: &InstallationId,
        artwork_id: &str,
    ) -> Result<bool, OwnWorkError> {
        Ok(self.file(installation_id, artwork_id)?.is_file())
    }

    fn read(
        &self,
        installation_id: &InstallationId,
        artwork_id: &str,
    ) -> Result<Vec<u8>, OwnWorkError> {
        let file = self.file(installation_id, artwork_id)?;
        fs::read(&file).map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => OwnWorkError::UnknownArtwork(artwork_id.to_owned()),
            _ => OwnWorkError::artwork(error),
        })
    }

    fn retain(
        &self,
        installation_id: &InstallationId,
        keep: &[String],
    ) -> Result<(), OwnWorkError> {
        let folder = self.folder(installation_id);
        let entries = match fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(OwnWorkError::artwork(error)),
        };
        let keep = keep.iter().map(String::as_str).collect::<HashSet<_>>();
        let now = SystemTime::now();
        for entry in entries {
            let path = entry.map_err(OwnWorkError::artwork)?.path();
            let kept = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .is_some_and(|stem| keep.contains(stem));
            let fresh = fs::metadata(&path)
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .is_none_or(|age| age < UNSAVED_GRACE);
            if !kept && !fresh {
                fs::remove_file(&path).map_err(OwnWorkError::artwork)?;
            }
        }
        Ok(())
    }

    fn remove_all_except(&self, installations: &[InstallationId]) -> Result<usize, OwnWorkError> {
        let entries = match fs::read_dir(&self.directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(OwnWorkError::artwork(error)),
        };
        let present = installations.iter().map(folder_key).collect::<HashSet<_>>();
        let mut removed = 0;
        for entry in entries {
            let entry = entry.map_err(OwnWorkError::artwork)?;
            let name = entry.file_name();
            let known = name.to_str().is_some_and(|name| present.contains(name));
            if !known && entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                fs::remove_dir_all(entry.path()).map_err(OwnWorkError::artwork)?;
                removed += 1;
            }
        }
        Ok(removed)
    }
}

fn folder_key(installation_id: &InstallationId) -> String {
    hex::encode(Sha256::digest(installation_id.0.as_bytes()))[..32].to_owned()
}

fn encode(source: &Path) -> Result<Vec<u8>, OwnWorkError> {
    let mut reader = ImageReader::open(source)
        .map_err(OwnWorkError::artwork)?
        .with_guessed_format()
        .map_err(OwnWorkError::artwork)?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SOURCE_EDGE);
    limits.max_image_height = Some(MAX_SOURCE_EDGE);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    let image = reader.decode().map_err(OwnWorkError::artwork)?;
    let image = if image.width() > ARTWORK_EDGE || image.height() > ARTWORK_EDGE {
        image.resize(ARTWORK_EDGE, ARTWORK_EDGE, FilterType::CatmullRom)
    } else {
        image
    };
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, ARTWORK_QUALITY)
        .encode_image(&over_black(image))
        .map_err(OwnWorkError::artwork)?;
    Ok(bytes)
}

// JPEG has no transparency, so a transparent picture is laid over black,
// which sits with the app's dark ground rather than standing out as a box.
fn over_black(image: DynamicImage) -> image::RgbImage {
    if !image.color().has_alpha() {
        return image.to_rgb8();
    }
    let rgba = image.to_rgba8();
    image::RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let [red, green, blue, alpha] = rgba.get_pixel(x, y).0;
        let scale = |channel: u8| ((u16::from(channel) * u16::from(alpha)) / 255) as u8;
        image::Rgb([scale(red), scale(green), scale(blue)])
    })
}

fn store(target: &Path, bytes: &[u8]) -> io::Result<()> {
    let folder = target.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(folder)?;
    let temporary = target.with_extension(format!(
        "{}-{}.tmp",
        std::process::id(),
        TEMPORARY_FILES.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, target).inspect_err(|_| {
        let _ = fs::remove_file(&temporary);
    })
}

#[cfg(test)]
mod tests {
    use image::{ImageFormat, RgbImage, RgbaImage};

    use super::*;

    fn picture(folder: &Path, name: &str, width: u32, height: u32) -> PathBuf {
        let path = folder.join(name);
        RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 40])
        })
        .save_with_format(&path, ImageFormat::Png)
        .expect("source picture");
        path
    }

    fn age(path: &Path) {
        fs::File::options()
            .write(true)
            .open(path)
            .and_then(|file| file.set_modified(SystemTime::now() - UNSAVED_GRACE * 2))
            .expect("aged file");
    }

    #[test]
    fn imports_a_shrunk_jpeg_named_by_its_digest_once() {
        let folder = tempfile::tempdir().expect("temporary folder");
        let store = OwnWorkArtworkFiles::new(folder.path().join("artwork"));
        let id = InstallationId("installation-1".to_owned());
        let source = picture(folder.path(), "cg_01.png", 3200, 1800);

        let artwork_id = store.import(&id, &source).expect("imported");
        let again = store.import(&id, &source).expect("imported again");

        assert_eq!(artwork_id, again);
        assert!(is_artwork_id(&artwork_id));
        assert!(store.contains(&id, &artwork_id).expect("contains"));
        let bytes = store.read(&id, &artwork_id).expect("read");
        assert_eq!(hex::encode(Sha256::digest(&bytes)), artwork_id);
        let stored = image::load_from_memory_with_format(&bytes, ImageFormat::Jpeg).expect("jpeg");
        assert_eq!((stored.width(), stored.height()), (1600, 900));
        assert!(
            !store
                .contains(&InstallationId("installation-2".to_owned()), &artwork_id)
                .expect("contains")
        );
    }

    #[test]
    fn keeps_a_small_picture_its_size_and_lays_transparency_over_black() {
        let folder = tempfile::tempdir().expect("temporary folder");
        let store = OwnWorkArtworkFiles::new(folder.path().join("artwork"));
        let id = InstallationId("installation-1".to_owned());
        let source = folder.path().join("icon.png");
        RgbaImage::from_pixel(64, 48, image::Rgba([200, 100, 50, 0]))
            .save_with_format(&source, ImageFormat::Png)
            .expect("transparent picture");

        let artwork_id = store.import(&id, &source).expect("imported");
        let stored = image::load_from_memory(&store.read(&id, &artwork_id).expect("read"))
            .expect("jpeg")
            .to_rgb8();

        assert_eq!(stored.dimensions(), (64, 48));
        assert!(
            stored
                .get_pixel(10, 10)
                .0
                .iter()
                .all(|channel| *channel < 8)
        );
    }

    #[test]
    fn refuses_files_that_are_not_pictures_and_ids_that_are_not_digests() {
        let folder = tempfile::tempdir().expect("temporary folder");
        let store = OwnWorkArtworkFiles::new(folder.path().join("artwork"));
        let id = InstallationId("installation-1".to_owned());
        let text = folder.path().join("notes.txt");
        fs::write(&text, "not a picture").expect("text file");

        assert!(matches!(
            store.import(&id, &text),
            Err(OwnWorkError::Artwork(_))
        ));
        assert!(matches!(
            store.read(&id, "../../secret"),
            Err(OwnWorkError::UnknownArtwork(_))
        ));
        assert!(matches!(
            store.read(&id, &"a".repeat(64)),
            Err(OwnWorkError::UnknownArtwork(_))
        ));
    }

    #[test]
    fn removes_unused_pictures_but_spares_a_fresh_import_and_other_works() {
        let folder = tempfile::tempdir().expect("temporary folder");
        let store = OwnWorkArtworkFiles::new(folder.path().join("artwork"));
        let id = InstallationId("installation-1".to_owned());
        let other = InstallationId("installation-2".to_owned());
        let kept = store
            .import(&id, &picture(folder.path(), "a.png", 40, 30))
            .expect("kept");
        let unused = store
            .import(&id, &picture(folder.path(), "b.png", 50, 30))
            .expect("unused");
        let fresh = store
            .import(&id, &picture(folder.path(), "c.png", 60, 30))
            .expect("fresh");
        let elsewhere = store
            .import(&other, &picture(folder.path(), "d.png", 70, 30))
            .expect("other");
        for artwork_id in [&kept, &unused] {
            age(&store.file(&id, artwork_id).expect("file"));
        }
        age(&store.file(&other, &elsewhere).expect("file"));

        store
            .retain(&id, std::slice::from_ref(&kept))
            .expect("retained");

        assert!(store.contains(&id, &kept).expect("contains"));
        assert!(!store.contains(&id, &unused).expect("contains"));
        assert!(store.contains(&id, &fresh).expect("contains"));
        assert!(store.contains(&other, &elsewhere).expect("contains"));

        assert_eq!(
            store
                .remove_all_except(std::slice::from_ref(&id))
                .expect("removed"),
            1
        );
        assert!(store.contains(&id, &kept).expect("contains"));
        assert!(!store.contains(&other, &elsewhere).expect("contains"));
    }
}
