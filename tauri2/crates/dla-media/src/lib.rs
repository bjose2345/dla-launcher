use std::path::Path;

use dla_application::media::{MediaError, MediaInventoryItem, MediaInventoryReader};
use dla_detection::classify_media_type;
use dla_domain::installation::{MediaType, RelativePath};
use walkdir::WalkDir;

mod waveform;

pub mod subtitles;

pub use subtitles::{
    SidecarSubtitle, SubtitleFormat, find_sidecar_subtitles, subtitle_to_web_vtt, to_web_vtt,
};
pub use waveform::DesktopAudioWaveformReader;

pub struct DesktopMediaInventory;

impl DesktopMediaInventory {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DesktopMediaInventory {
    fn default() -> Self {
        Self::new()
    }
}

impl MediaInventoryReader for DesktopMediaInventory {
    fn read_inventory(&self, root_path: &str) -> Result<Vec<MediaInventoryItem>, MediaError> {
        let root = Path::new(root_path);
        let mut items = WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_map(|entry| match entry {
                Ok(entry) if entry.file_type().is_file() && !entry.path_is_symlink() => {
                    Some(inventory_item(root, entry.path()).map_err(MediaError::inventory))
                }
                Ok(_) => None,
                Err(error) => Some(Err(MediaError::inventory(error))),
            })
            .collect::<Result<Vec<_>, _>>()?;
        items.retain(|item| {
            matches!(
                item.media_type,
                MediaType::Audio | MediaType::Image | MediaType::Pdf | MediaType::Video
            )
        });
        items.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        Ok(items)
    }
}

fn inventory_item(root: &Path, path: &Path) -> Result<MediaInventoryItem, String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|error| error.to_string())?
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    let relative_path = RelativePath::parse(relative).map_err(|error| error.to_string())?;
    let size_bytes = path.metadata().map_err(|error| error.to_string())?.len();
    Ok(MediaInventoryItem {
        media_type: classify_media_type(&relative_path),
        relative_path,
        size_bytes: Some(size_bytes),
    })
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::{Arc, Mutex},
    };

    use dla_application::{
        installation::{InstallationLibraryError, InstallationStore},
        media::{AudioTrackStore, MediaService, MediaSessionStore, OpenMediaSessionRequest},
        package_preparation::{PackagePreparationError, PackagePreparationStore},
    };
    use dla_domain::{
        installation::{
            Installation, InstallationDetection, InstallationId, InstallationOverrides,
            InstallationPlatform, InstallationStatus, LaunchActionKind,
        },
        media::{
            IndexedAudioTrack, MediaQueueState, MediaRepeatMode, MediaResume, MediaSession,
            MediaSessionId, MediaSessionKind,
        },
        package::{
            ArchiveRetentionPolicy, PackageSourceSet, PackageSourceSetKind,
            PreparedPackageInstallation,
        },
    };
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn inventory_is_portable_sorted_and_media_only() {
        let directory = tempdir().expect("temporary directory");
        fs::create_dir_all(directory.path().join("disc")).expect("disc");
        fs::write(directory.path().join("disc/02.flac"), b"two").expect("track two");
        fs::write(directory.path().join("disc/01.flac"), b"one").expect("track one");
        fs::write(directory.path().join("notes.txt"), b"ignore").expect("notes");

        let items = DesktopMediaInventory::new()
            .read_inventory(directory.path().to_str().expect("path"))
            .expect("inventory");

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].relative_path.as_str(), "disc/01.flac");
        assert_eq!(items[1].relative_path.as_str(), "disc/02.flac");
        assert!(items.iter().all(|item| item.media_type == MediaType::Audio));
    }

    #[cfg(unix)]
    #[test]
    fn inventory_does_not_follow_symbolic_links() {
        use std::os::unix::fs::symlink;

        let root = tempdir().expect("root");
        let outside = tempdir().expect("outside");
        fs::write(outside.path().join("private.mp3"), b"outside").expect("outside track");
        symlink(outside.path(), root.path().join("escape")).expect("symlink");

        assert!(
            DesktopMediaInventory::new()
                .read_inventory(root.path().to_str().expect("path"))
                .expect("inventory")
                .is_empty()
        );
    }

    struct FixedInstallationStore(Installation);

    impl InstallationStore for FixedInstallationStore {
        fn create(&self, _: &Installation) -> Result<(), InstallationLibraryError> {
            unreachable!()
        }

        fn create_or_refresh(
            &self,
            _: &Installation,
        ) -> Result<Installation, InstallationLibraryError> {
            unreachable!()
        }

        fn read(
            &self,
            installation_id: &InstallationId,
        ) -> Result<Option<Installation>, InstallationLibraryError> {
            Ok((self.0.id == *installation_id).then(|| self.0.clone()))
        }

        fn list(&self) -> Result<Vec<Installation>, InstallationLibraryError> {
            Ok(vec![self.0.clone()])
        }

        fn replace_detection(
            &self,
            _: &InstallationId,
            _: &InstallationDetection,
            _: InstallationStatus,
            _: &str,
        ) -> Result<(), InstallationLibraryError> {
            unreachable!()
        }

        fn replace_overrides(
            &self,
            _: &InstallationId,
            _: &InstallationOverrides,
            _: InstallationStatus,
            _: &str,
        ) -> Result<(), InstallationLibraryError> {
            unreachable!()
        }
    }

    struct FixedPreparationStore(PreparedPackageInstallation);

    impl PackagePreparationStore for FixedPreparationStore {
        fn read_prepared_package(
            &self,
            installation_id: &InstallationId,
        ) -> Result<Option<PreparedPackageInstallation>, PackagePreparationError> {
            Ok((self.0.installation_id == *installation_id).then(|| self.0.clone()))
        }

        fn save_prepared_package(
            &self,
            _: &PreparedPackageInstallation,
        ) -> Result<(), PackagePreparationError> {
            unreachable!()
        }
    }

    #[derive(Default)]
    struct MemoryMediaStore {
        session: Mutex<Option<MediaSession>>,
        tracks: Mutex<Vec<IndexedAudioTrack>>,
    }

    impl MediaSessionStore for MemoryMediaStore {
        fn create_media_session(
            &self,
            session: &MediaSession,
        ) -> Result<(), dla_application::media::MediaError> {
            *self.session.lock().expect("session") = Some(session.clone());
            Ok(())
        }

        fn save_media_session(
            &self,
            session: &MediaSession,
        ) -> Result<(), dla_application::media::MediaError> {
            *self.session.lock().expect("session") = Some(session.clone());
            Ok(())
        }

        fn read_media_session(
            &self,
            session_id: &MediaSessionId,
        ) -> Result<Option<MediaSession>, dla_application::media::MediaError> {
            Ok(self
                .session
                .lock()
                .expect("session")
                .as_ref()
                .filter(|session| session.id == *session_id)
                .cloned())
        }

        fn read_open_media_session(
            &self,
            installation_id: &InstallationId,
        ) -> Result<Option<MediaSession>, dla_application::media::MediaError> {
            Ok(self
                .session
                .lock()
                .expect("session")
                .as_ref()
                .filter(|session| {
                    session.installation_id == *installation_id && session.status.is_open()
                })
                .cloned())
        }

        fn read_open_personalized_media_session(
            &self,
        ) -> Result<Option<MediaSession>, dla_application::media::MediaError> {
            Ok(None)
        }

        fn read_media_queue_state(
            &self,
            _: MediaSessionKind,
            _: Option<&InstallationId>,
        ) -> Result<Option<MediaQueueState>, dla_application::media::MediaError> {
            Ok(None)
        }

        fn read_media_resume(
            &self,
            _: &InstallationId,
            _: LaunchActionKind,
        ) -> Result<Option<MediaResume>, dla_application::media::MediaError> {
            Ok(None)
        }

        fn interrupt_open_media_sessions(
            &self,
            _: &str,
            _: &str,
        ) -> Result<u64, dla_application::media::MediaError> {
            Ok(0)
        }
    }

    impl AudioTrackStore for MemoryMediaStore {
        fn replace_audio_tracks(
            &self,
            _: &InstallationId,
            tracks: &[IndexedAudioTrack],
        ) -> Result<(), dla_application::media::MediaError> {
            *self.tracks.lock().expect("tracks") = tracks.to_vec();
            Ok(())
        }

        fn list_audio_tracks(
            &self,
            _: &InstallationId,
        ) -> Result<Vec<IndexedAudioTrack>, dla_application::media::MediaError> {
            Ok(self.tracks.lock().expect("tracks").clone())
        }

        fn list_all_audio_tracks(
            &self,
        ) -> Result<Vec<IndexedAudioTrack>, dla_application::media::MediaError> {
            Ok(self.tracks.lock().expect("tracks").clone())
        }
    }

    #[test]
    fn opens_a_real_prepared_audio_tree_with_unicode_filenames() {
        let directory = tempdir().expect("temporary directory");
        for (relative, bytes) in [
            ("mp3/sa02_01_魔王城にて再会.mp3", b"first".as_slice()),
            ("mp3/sa02_02_右の耳かき.mp3", b"second".as_slice()),
            ("wav/sa02_01_魔王城にて再会.wav", b"lossless".as_slice()),
            ("omake/イラスト.jpg", b"image".as_slice()),
        ] {
            let path = directory.path().join(relative);
            fs::create_dir_all(path.parent().expect("parent")).expect("media directory");
            fs::write(path, bytes).expect("media file");
        }
        let installation_id = InstallationId("installation-unicode-audio".to_owned());
        let installation = Installation {
            id: installation_id.clone(),
            scan_root_id: None,
            root_path: "/synthetic/source".to_owned(),
            platform: InstallationPlatform::Linux,
            status: InstallationStatus::Ready,
            detection: InstallationDetection {
                source_scan_session_id: None,
                catalog_identity: None,
                suggested_status: InstallationStatus::Ready,
                content_items: Vec::new(),
                launch_candidates: Vec::new(),
                package_inspection: None,
            },
            overrides: InstallationOverrides {
                catalog_identity: None,
                custom_title: None,
                preferred_action: None,
                content_items: Vec::new(),
                reviewed_at: Some("2026-08-13T00:00:00Z".to_owned()),
            },
            discovered_at: "2026-08-13T00:00:00Z".to_owned(),
            updated_at: "2026-08-13T00:00:00Z".to_owned(),
        };
        let prepared = PreparedPackageInstallation {
            installation_id: installation_id.clone(),
            destination_root: directory.path().to_string_lossy().into_owned(),
            content_root: None,
            preferred_action: None,
            source_set: PackageSourceSet {
                kind: PackageSourceSetKind::SingleArchive,
                volumes: Vec::new(),
            },
            archive_retention: ArchiveRetentionPolicy::Keep,
            sources_deleted: false,
            source_cleanup_error: None,
            installed_file_count: 4,
            installed_bytes: 25,
            prepared_at: "2026-08-13T00:00:00Z".to_owned(),
        };
        let media_store = Arc::new(MemoryMediaStore::default());
        let service = MediaService::new(
            Arc::new(FixedInstallationStore(installation)),
            Arc::new(FixedPreparationStore(prepared)),
            media_store.clone(),
            media_store,
            Arc::new(DesktopMediaInventory::new()),
            Arc::new(DesktopAudioWaveformReader::new(
                directory.path().join("waveforms"),
            )),
        );

        let session = service
            .open(OpenMediaSessionRequest {
                installation_id,
                session_id: MediaSessionId("session-unicode-audio".to_owned()),
                opened_at: "2026-08-13T01:00:00Z".to_owned(),
            })
            .expect("open media session");

        assert_eq!(session.action, LaunchActionKind::PlayAudio);
        assert_eq!(session.repeat_mode, MediaRepeatMode::Off);
        assert_eq!(session.items.len(), 2);
        assert!(
            session
                .items
                .iter()
                .all(|item| item.relative_path.as_str().starts_with("mp3/"))
        );
    }
}
