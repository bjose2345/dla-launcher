use std::sync::Arc;

use dla_application::recommendation::{CatalogRecommendationService, CatalogRecommendations};
use dla_application::{
    android_app::AndroidAppService,
    android_package::AndroidPackageService,
    catalog::{BrowseRequest, CatalogContext, CatalogContextRequest, CatalogPage, CatalogService},
    catalog_artwork::{
        CatalogArtworkCache, CatalogArtworkCacheSummary, CatalogArtworkCapacity,
        CatalogArtworkRetention,
    },
    catalog_import::{CatalogGenerationSummary, CatalogImportPreview, CatalogImportProgress},
    installation_review::{InstallationReviewRequest, InstallationReviewService},
    launch::{LaunchInstallationRequest, LaunchOptionStore, LaunchOptions, LaunchService},
    library_shelves::LibraryShelvesService,
    maintenance::LibraryMaintenanceService,
    media::{
        AudioWaveform, MediaService, OpenMediaSessionRequest, OpenPersonalizedVoiceQueueRequest,
        PersonalizedVoiceQueueService, UpdateMediaProgressRequest, UpdateMediaQueueSettingsRequest,
    },
    own_work::OwnWorkService,
    package_preparation::{PackageDestinationConflictPolicy, PackageDestinationPreview},
    personalization::{LocalPersonalizationService, WorkPreferenceService},
    scan_findings::{AddScanFindingsRequest, ScanFindingList, ScanFindingService},
    scanner::{ScanIssuePage, ScanIssueRequest, ScanRootPreference, ScanSessionView},
    search::{
        CatalogSearchService, SearchCacheCleanupReport, SearchIndexStatus, SearchRebuildProgress,
        SearchRequest, SearchResponse, SearchShortcut, SearchShortcutRequest,
    },
};
#[cfg(desktop)]
use dla_catalog_import::CATALOG_PACKAGE_FILE_EXTENSIONS;
use dla_domain::{
    CatalogRomContents, CatalogWork, CatalogWorkDetail,
    android_app::{AndroidAppAssociationId, AndroidAppView},
    installation::{Installation, InstallationId},
    launch::{LaunchActivity, LaunchActivityId, WindowScale},
    library::{LibraryShelves, LocalPersonalization, WorkPreference, WorkPreferenceKind},
    maintenance::{
        FileNameReport, InstallationHealthReport, LibraryFileNameReport, MaintenanceCleanupReport,
    },
    media::{MediaRepeatMode, MediaSession, MediaSessionId, MediaSessionItem, MediaSessionStatus},
    own_work::{
        ImportOwnWorkArtworkRequest, InstallationImage, OwnWorkDetails, SaveOwnWorkDetailsRequest,
    },
    package::{ArchiveRetentionPolicy, PackagePreparationProgress, PreparedPackageInstallation},
    scanner::{ScanRootId, ScanSessionId},
};
#[cfg(desktop)]
use dla_scanner::ApprovedScanRoot;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, LogicalSize, WebviewWindow};
use uuid::Uuid;

use crate::{
    catalog_import::CatalogImportController,
    native_video::{
        NativeVideoControlRequest, NativeVideoController, NativeVideoRectangle,
        NativeVideoViewport, OpenNativeVideoRequest, OpenNativeVideoResponse,
    },
    package_preparation::PackagePreparationController,
    scan_installs::{ScanAddResponse, ScanInstallController, ScanInstallView},
    scanner::ScanController,
    search::SearchIndexController,
};

pub(crate) async fn blocking_value<T>(
    operation: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String>
where
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(display_error)
}

pub(crate) async fn blocking<T, E>(
    operation: impl FnOnce() -> Result<T, E> + Send + 'static,
) -> Result<T, String>
where
    T: Send + 'static,
    E: std::fmt::Display + Send + 'static,
{
    blocking_value(operation).await?.map_err(display_error)
}

/// Shows the folder picker off the main thread; `None` when it is closed
/// without a choice.
#[cfg(desktop)]
async fn pick_folder(
    app: AppHandle,
    title: &'static str,
    initial_directory: Option<String>,
) -> Result<Option<std::path::PathBuf>, String> {
    use tauri_plugin_dialog::DialogExt;

    let picked = blocking_value(move || {
        let dialog = app.dialog().file().set_title(title);
        match initial_directory {
            Some(directory) => dialog.set_directory(directory).blocking_pick_folder(),
            None => dialog.blocking_pick_folder(),
        }
    })
    .await?;
    picked_path(picked)
}

/// Shows the file picker off the main thread; `None` when it is closed
/// without a choice.
#[cfg(desktop)]
async fn pick_file(
    app: AppHandle,
    title: &'static str,
    filter: Option<(&'static str, &'static [&'static str])>,
) -> Result<Option<std::path::PathBuf>, String> {
    use tauri_plugin_dialog::DialogExt;

    let picked = blocking_value(move || {
        let dialog = app.dialog().file().set_title(title);
        match filter {
            Some((name, extensions)) => dialog.add_filter(name, extensions).blocking_pick_file(),
            None => dialog.blocking_pick_file(),
        }
    })
    .await?;
    picked_path(picked)
}

#[cfg(desktop)]
fn picked_path(
    picked: Option<tauri_plugin_dialog::FilePath>,
) -> Result<Option<std::path::PathBuf>, String> {
    picked
        .map(|path| path.into_path().map_err(display_error))
        .transpose()
}

/// Where the install folder pickers start: the configured default install
/// root, when it exists.
#[cfg(desktop)]
fn default_install_root() -> Option<String> {
    std::env::var("DLA_DEFAULT_INSTALL_ROOT")
        .ok()
        .filter(|path| std::path::Path::new(path).is_dir())
}

pub struct AppState {
    pub android_app: Arc<AndroidAppService>,
    pub android_package: Arc<AndroidPackageService>,
    pub catalog: Arc<CatalogService>,
    pub cover_cache: Arc<dyn CatalogArtworkCache>,
    pub recommendations: Arc<CatalogRecommendationService>,
    pub search: Arc<CatalogSearchService>,
    pub search_index_controller: Arc<SearchIndexController>,
    pub scanner: Arc<ScanController>,
    pub catalog_import: Arc<CatalogImportController>,
    pub scan_findings: Arc<ScanFindingService>,
    pub scan_installs: Arc<ScanInstallController>,
    #[cfg(desktop)]
    pub library_add: Arc<crate::library_add::LibraryAddController>,
    pub installation_review: Arc<InstallationReviewService>,
    pub library_shelves: Arc<LibraryShelvesService>,
    pub work_preferences: Arc<WorkPreferenceService>,
    pub own_works: Arc<OwnWorkService>,
    pub local_personalization: Arc<LocalPersonalizationService>,
    pub package_preparation: Arc<PackagePreparationController>,
    pub maintenance: Arc<LibraryMaintenanceService>,
    pub launch: Arc<LaunchService>,
    pub launch_options: Arc<dyn LaunchOptionStore>,
    pub locale_remulator: dla_launch::LocaleRemulator,
    pub translation: Arc<crate::translation::TranslationController>,
    pub media: Arc<MediaService>,
    pub personalized_voice: Arc<PersonalizedVoiceQueueService>,
    pub native_video: Arc<NativeVideoController>,
    #[cfg(desktop)]
    pub delivery: Arc<crate::delivery::DeliveryController>,
}

#[tauri::command]
pub async fn list_android_apps(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<AndroidAppView>, String> {
    let service = Arc::clone(&state.android_app);
    blocking(move || service.list()).await
}

#[tauri::command]
pub async fn associate_installed_android_app(
    state: tauri::State<'_, AppState>,
    work_code: String,
) -> Result<AndroidAppView, String> {
    let app_service = Arc::clone(&state.android_app);
    let package_service = Arc::clone(&state.android_package);
    blocking(move || {
        let package_state = package_service.read_state().map_err(display_error)?;
        app_service
            .associate_installed(
                &work_code,
                AndroidAppAssociationId(format!("android-app-{}", Uuid::new_v4())),
                &dla_sqlite::current_timestamp(),
                &package_state,
            )
            .map_err(display_error)
    })
    .await
}

#[tauri::command]
pub async fn launch_android_app(
    state: tauri::State<'_, AppState>,
    association_id: String,
) -> Result<AndroidAppView, String> {
    let service = Arc::clone(&state.android_app);
    blocking(move || {
        service.launch(
            &AndroidAppAssociationId(association_id),
            &dla_sqlite::current_timestamp(),
        )
    })
    .await
}

#[tauri::command]
pub async fn remove_android_app_association(
    state: tauri::State<'_, AppState>,
    association_id: String,
) -> Result<(), String> {
    let service = Arc::clone(&state.android_app);
    blocking(move || service.remove(&AndroidAppAssociationId(association_id))).await
}

#[tauri::command]
pub async fn read_android_package_state(
    state: tauri::State<'_, AppState>,
) -> Result<dla_domain::android_package::AndroidPackageState, String> {
    let service = Arc::clone(&state.android_package);
    blocking(move || service.read_state()).await
}

#[tauri::command]
pub async fn select_and_inspect_android_package(
    state: tauri::State<'_, AppState>,
) -> Result<dla_domain::android_package::AndroidPackageState, String> {
    let service = Arc::clone(&state.android_package);
    blocking(move || service.select_and_inspect()).await
}

#[tauri::command]
pub async fn clear_android_package_selection(
    state: tauri::State<'_, AppState>,
) -> Result<dla_domain::android_package::AndroidPackageState, String> {
    let service = Arc::clone(&state.android_package);
    blocking(move || service.clear_selection()).await
}

#[tauri::command]
pub async fn open_android_package_source_approval(
    state: tauri::State<'_, AppState>,
) -> Result<dla_domain::android_package::AndroidPackageState, String> {
    let service = Arc::clone(&state.android_package);
    blocking(move || service.open_source_approval()).await
}

#[tauri::command]
pub async fn request_android_package_install(
    state: tauri::State<'_, AppState>,
) -> Result<dla_domain::android_package::AndroidPackageState, String> {
    let service = Arc::clone(&state.android_package);
    blocking(move || service.request_install()).await
}

#[tauri::command]
pub async fn read_cover_cache_summary(
    state: tauri::State<'_, AppState>,
) -> Result<CatalogArtworkCacheSummary, String> {
    state.cover_cache.summary().map_err(display_error)
}

#[tauri::command]
pub async fn configure_cover_cache(
    state: tauri::State<'_, AppState>,
    retention: CatalogArtworkRetention,
    capacity: CatalogArtworkCapacity,
) -> Result<CatalogArtworkCacheSummary, String> {
    let cache = Arc::clone(&state.cover_cache);
    blocking(move || cache.configure(retention, capacity)).await
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedInstallationDestination {
    access_handle: String,
    display_path: String,
}

#[cfg(desktop)]
#[tauri::command]
pub async fn select_installation_destination(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Option<SelectedInstallationDestination>, String> {
    let Some(path) = pick_folder(
        app,
        "Choose where this work will be installed",
        default_install_root(),
    )
    .await?
    else {
        return Ok(None);
    };
    approved_destination(&state, &path).map(Some)
}

#[cfg(desktop)]
#[tauri::command]
pub async fn select_installation_location(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Option<SelectedInstallationDestination>, String> {
    let Some(path) = pick_folder(app, "Locate the existing installation folder", None).await?
    else {
        return Ok(None);
    };
    approved_destination(&state, &path).map(Some)
}

#[cfg(desktop)]
fn approved_destination(
    state: &AppState,
    path: &std::path::Path,
) -> Result<SelectedInstallationDestination, String> {
    let approved = state
        .package_preparation
        .approve_destination(path)
        .map_err(display_error)?;
    Ok(SelectedInstallationDestination {
        access_handle: approved.access_handle,
        display_path: approved.display_path,
    })
}

#[cfg(mobile)]
#[tauri::command]
pub async fn select_installation_location(
    _app: AppHandle,
    _state: tauri::State<'_, AppState>,
) -> Result<Option<SelectedInstallationDestination>, String> {
    Err("library relocation is not available on this platform yet".to_owned())
}

#[tauri::command]
pub async fn read_installation_health(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<InstallationHealthReport, String> {
    let service = Arc::clone(&state.maintenance);
    blocking(move || service.read_health(&InstallationId(installation_id))).await
}

#[tauri::command]
pub async fn read_installation_healths(
    state: tauri::State<'_, AppState>,
    installation_ids: Vec<String>,
) -> Result<Vec<InstallationHealthReport>, String> {
    let service = Arc::clone(&state.maintenance);
    blocking(move || {
        let installation_ids = installation_ids
            .into_iter()
            .map(InstallationId)
            .collect::<Vec<_>>();
        service.read_healths(&installation_ids)
    })
    .await
}

#[tauri::command]
pub async fn verify_library_installation(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<InstallationHealthReport, String> {
    let installation_id = InstallationId(installation_id);
    ensure_not_preparing(&state.package_preparation, &installation_id)?;
    let service = Arc::clone(&state.maintenance);
    blocking(move || service.verify(&installation_id, dla_sqlite::current_timestamp())).await
}

#[tauri::command]
pub async fn locate_library_installation(
    state: tauri::State<'_, AppState>,
    installation_id: String,
    location_access_handle: String,
) -> Result<InstallationHealthReport, String> {
    let installation_id = InstallationId(installation_id);
    ensure_not_preparing(&state.package_preparation, &installation_id)?;
    let selected_root = state
        .package_preparation
        .resolve_destination(&location_access_handle)
        .map_err(display_error)?;
    let service = Arc::clone(&state.maintenance);
    blocking(move || {
        service.relocate(
            &installation_id,
            selected_root,
            dla_sqlite::current_timestamp(),
        )
    })
    .await
}

#[tauri::command]
pub async fn rescan_library_installation(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<InstallationHealthReport, String> {
    let installation_id = InstallationId(installation_id);
    ensure_not_preparing(&state.package_preparation, &installation_id)?;
    let service = Arc::clone(&state.maintenance);
    blocking(move || service.rescan(&installation_id, dla_sqlite::current_timestamp())).await
}

#[tauri::command]
pub async fn repair_library_installation(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<InstallationHealthReport, String> {
    let installation_id = InstallationId(installation_id);
    ensure_not_preparing(&state.package_preparation, &installation_id)?;
    let service = Arc::clone(&state.maintenance);
    blocking(move || {
        service.repair(
            &installation_id,
            Uuid::new_v4().to_string(),
            dla_sqlite::current_timestamp(),
        )
    })
    .await
}

#[tauri::command]
pub async fn remove_library_installation(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<(), String> {
    let installation_id = InstallationId(installation_id);
    ensure_not_preparing(&state.package_preparation, &installation_id)?;
    let service = Arc::clone(&state.maintenance);
    let translation = Arc::clone(&state.translation);
    blocking(move || {
        // The game stays where it is, so it goes back to how it was.
        if let Err(error) = translation.games().remove(&installation_id) {
            log::warn!(target: "dla::translation", "event=translation_removal_failed installation_id={} error={error}", installation_id.0);
        }
        service.remove_from_library(&installation_id)
    })
    .await
}

#[tauri::command]
pub async fn uninstall_library_installation(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<(), String> {
    let installation_id = InstallationId(installation_id);
    ensure_not_preparing(&state.package_preparation, &installation_id)?;
    let service = Arc::clone(&state.maintenance);
    blocking(move || service.uninstall(&installation_id)).await
}

/// Opens a Library work's own folder in the system file manager: the folder
/// it was installed to, or its folder inside the scanned root.
#[cfg(desktop)]
#[tauri::command]
pub async fn open_installation_folder(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;

    let installation_id = InstallationId(installation_id);
    let review = Arc::clone(&state.installation_review);
    let media = Arc::clone(&state.media);
    let folder = blocking(move || -> Result<std::path::PathBuf, String> {
        if let Some(prepared) = media
            .read_prepared_package(&installation_id)
            .map_err(display_error)?
        {
            return Ok(std::path::PathBuf::from(prepared.destination_root));
        }
        let installation = review.read(&installation_id).map_err(display_error)?;
        Ok(installation_folder(&installation))
    })
    .await?;
    if !folder.is_dir() {
        return Err(format!("the folder no longer exists: {}", folder.display()));
    }
    app.opener()
        .open_path(folder.to_string_lossy(), None::<&str>)
        .map_err(display_error)
}

#[cfg(desktop)]
fn installation_folder(installation: &Installation) -> std::path::PathBuf {
    let root = std::path::PathBuf::from(&installation.root_path);
    if installation.detection.package_inspection.is_some() {
        return root;
    }
    let paths = installation
        .detection
        .content_items
        .iter()
        .map(|item| item.relative_path.as_str())
        .collect::<Vec<_>>();
    let Some((first, _)) = paths.first().and_then(|path| path.split_once('/')) else {
        return root;
    };
    let shared = paths
        .iter()
        .all(|path| path.split_once('/').is_some_and(|(top, _)| top == first));
    if shared { root.join(first) } else { root }
}

#[cfg(mobile)]
#[tauri::command]
pub async fn open_installation_folder(_installation_id: String) -> Result<(), String> {
    Err("opening folders is available on Windows and Linux".to_owned())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchOptionsView {
    japanese_locale: bool,
    /// Only Windows needs it: Wine always runs games in Japanese.
    japanese_locale_available: bool,
    /// The Visual C++ runtimes ("x64", "x86") Locale Remulator needs and
    /// this Windows lacks.
    missing_runtimes: Vec<&'static str>,
    /// A video player program of the user's for the work's videos.
    video_player: Option<String>,
    /// An audio player program of the user's for the work's tracks.
    audio_player: Option<String>,
    /// How Linux shows the game's window; none leaves it as the game made it.
    window_scale: Option<WindowScale>,
    /// Linux with gamescope installed can enlarge a game's window.
    window_scale_available: bool,
    /// The refresh rate of the enlarged game, in Hz; none is the default.
    refresh_rate: Option<u32>,
}

/// Which of a work's media a player program is for.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaPlayerKind {
    Audio,
    Video,
}

/// A chosen player program, or none; one that is not on disk is refused.
fn chosen_program(program: Option<String>) -> Result<Option<String>, String> {
    let program = program
        .map(|program| program.trim().to_owned())
        .filter(|program| !program.is_empty());
    if let Some(program) = &program
        && !std::path::Path::new(program).is_file()
    {
        return Err("the player program was not found".to_owned());
    }
    Ok(program)
}

#[tauri::command]
pub async fn read_launch_options(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<LaunchOptionsView, String> {
    let store = Arc::clone(&state.launch_options);
    blocking(move || {
        store
            .read_launch_options(&InstallationId(installation_id))
            .map(launch_options_view)
    })
    .await
}

/// Turning the Japanese locale on fetches Locale Remulator at once, so the
/// game starts without a wait and a failed download shows here.
#[tauri::command]
pub async fn save_launch_options(
    state: tauri::State<'_, AppState>,
    installation_id: String,
    japanese_locale: bool,
    video_player: Option<String>,
    audio_player: Option<String>,
    window_scale: Option<WindowScale>,
    refresh_rate: Option<u32>,
) -> Result<LaunchOptionsView, String> {
    if refresh_rate.is_some_and(|rate| !(30..=240).contains(&rate)) {
        return Err("the refresh rate must be between 30 and 240 Hz".to_owned());
    }
    let store = Arc::clone(&state.launch_options);
    let locale = state.locale_remulator.clone();
    let video_player = chosen_program(video_player)?;
    let audio_player = chosen_program(audio_player)?;
    blocking(move || {
        if japanese_locale && cfg!(target_os = "windows") {
            locale.install()?;
        }
        let options = LaunchOptions {
            japanese_locale,
            video_player,
            audio_player,
            window_scale,
            refresh_rate,
        };
        store.save_launch_options(&InstallationId(installation_id), options.clone())?;
        Ok::<_, dla_application::launch::LaunchError>(launch_options_view(options))
    })
    .await
}

fn launch_options_view(options: LaunchOptions) -> LaunchOptionsView {
    LaunchOptionsView {
        japanese_locale: options.japanese_locale,
        japanese_locale_available: cfg!(target_os = "windows"),
        missing_runtimes: dla_launch::LocaleRemulator::missing_runtimes()
            .into_iter()
            .map(dla_launch::VisualCppRuntime::as_str)
            .collect(),
        video_player: options.video_player,
        audio_player: options.audio_player,
        window_scale: options.window_scale,
        window_scale_available: dla_launch::window_scale_available(),
        refresh_rate: options.refresh_rate,
    }
}

/// Lets the user pick the program that plays a work's videos or tracks.
#[cfg(desktop)]
#[tauri::command]
pub async fn select_media_player(app: AppHandle) -> Result<Option<String>, String> {
    let filter = cfg!(target_os = "windows").then_some(("Programs", PROGRAM_EXTENSIONS));
    Ok(pick_file(app, "Choose a player", filter)
        .await?
        .map(|path| path.to_string_lossy().into_owned()))
}

#[cfg(desktop)]
const PROGRAM_EXTENSIONS: &[&str] = &["exe"];

#[cfg(mobile)]
#[tauri::command]
pub async fn select_media_player(_app: AppHandle) -> Result<Option<String>, String> {
    Err("choosing a player is not available on this platform".to_owned())
}

/// Plays a work's videos or tracks in the player chosen for them, the given
/// one first and the rest after it, as a playlist.
#[cfg(desktop)]
#[tauri::command]
pub async fn play_in_player(
    state: tauri::State<'_, AppState>,
    installation_id: String,
    kind: MediaPlayerKind,
    ordinal: Option<u32>,
) -> Result<(), String> {
    use dla_domain::installation::MediaType;

    let store = Arc::clone(&state.launch_options);
    let media = Arc::clone(&state.media);
    blocking(move || {
        let installation_id = InstallationId(installation_id);
        let options = store
            .read_launch_options(&installation_id)
            .map_err(display_error)?;
        let (player, media_type) = match kind {
            MediaPlayerKind::Audio => (options.audio_player, MediaType::Audio),
            MediaPlayerKind::Video => (options.video_player, MediaType::Video),
        };
        let player = player.ok_or_else(|| "this work has no player chosen".to_owned())?;
        let (root, items) = media
            .list_items_in_root(&installation_id, &dla_sqlite::current_timestamp())
            .map_err(display_error)?;
        let root = std::path::Path::new(&root);
        let mut files: Vec<std::path::PathBuf> = items
            .iter()
            .filter(|item| item.media_type == media_type)
            .map(|item| root.join(item.relative_path.as_str()))
            .collect();
        if let Some(ordinal) = ordinal
            && let Some(first) = items
                .iter()
                .find(|item| item.ordinal == ordinal)
                .map(|item| root.join(item.relative_path.as_str()))
            && let Some(at) = files.iter().position(|path| *path == first)
        {
            files.rotate_left(at);
        }
        if files.is_empty() {
            return Err("this work has nothing to play".to_owned());
        }
        let mut child = std::process::Command::new(&player)
            .args(&files)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|error| format!("the player could not start: {error}"))?;
        // Reaped when it ends, so it does not linger as a zombie; the
        // launcher does not otherwise follow it.
        std::thread::Builder::new()
            .name("dla-media-player".to_owned())
            .spawn(move || {
                let _ = child.wait();
            })
            .map_err(display_error)?;
        Ok(())
    })
    .await
}

#[cfg(mobile)]
#[tauri::command]
pub async fn play_in_player(
    _state: tauri::State<'_, AppState>,
    _installation_id: String,
    _kind: MediaPlayerKind,
    _ordinal: Option<u32>,
) -> Result<(), String> {
    Err("playing in another player is not available on this platform".to_owned())
}

#[tauri::command]
pub async fn cleanup_library_maintenance(
    state: tauri::State<'_, AppState>,
) -> Result<MaintenanceCleanupReport, String> {
    ensure_nothing_preparing(&state.package_preparation)?;
    let service = Arc::clone(&state.maintenance);
    blocking(move || service.cleanup_abandoned()).await
}

#[tauri::command]
pub async fn check_library_file_names(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<FileNameReport, String> {
    let installation_id = InstallationId(installation_id);
    let service = Arc::clone(&state.maintenance);
    blocking(move || service.check_file_names(&installation_id)).await
}

#[tauri::command]
pub async fn fix_library_file_names(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<FileNameReport, String> {
    let installation_id = InstallationId(installation_id);
    ensure_not_preparing(&state.package_preparation, &installation_id)?;
    let service = Arc::clone(&state.maintenance);
    blocking(move || service.fix_file_names(&installation_id, dla_sqlite::current_timestamp()))
        .await
}

#[tauri::command]
pub async fn fix_all_library_file_names(
    state: tauri::State<'_, AppState>,
) -> Result<LibraryFileNameReport, String> {
    ensure_nothing_preparing(&state.package_preparation)?;
    let service = Arc::clone(&state.maintenance);
    blocking(move || service.fix_all_file_names(dla_sqlite::current_timestamp())).await
}

fn ensure_not_preparing(
    controller: &PackagePreparationController,
    installation_id: &InstallationId,
) -> Result<(), String> {
    if controller.installation_is_active(installation_id) {
        Err("package preparation is currently active for this installation".to_owned())
    } else {
        Ok(())
    }
}

fn ensure_nothing_preparing(controller: &PackagePreparationController) -> Result<(), String> {
    if controller.has_active_operation() {
        Err("package preparation is currently active".to_owned())
    } else {
        Ok(())
    }
}

#[cfg(mobile)]
#[tauri::command]
pub async fn select_installation_destination(
    _app: AppHandle,
    _state: tauri::State<'_, AppState>,
) -> Result<Option<SelectedInstallationDestination>, String> {
    Err("package preparation is not available on this platform yet".to_owned())
}

#[tauri::command]
pub async fn read_prepared_package(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<Option<PreparedPackageInstallation>, String> {
    let service = Arc::clone(&state.media);
    blocking(move || service.read_prepared_package(&InstallationId(installation_id))).await
}

#[tauri::command]
pub async fn read_prepared_packages(
    state: tauri::State<'_, AppState>,
    installation_ids: Vec<String>,
) -> Result<Vec<PreparedPackageInstallation>, String> {
    let service = Arc::clone(&state.media);
    blocking(move || {
        let installation_ids = installation_ids
            .into_iter()
            .map(InstallationId)
            .collect::<Vec<_>>();
        service.read_prepared_packages(&installation_ids)
    })
    .await
}

#[tauri::command]
pub async fn start_package_preparation(
    state: tauri::State<'_, AppState>,
    installation_id: String,
    destination_access_handle: String,
    destination_conflict_policy: PackageDestinationConflictPolicy,
    archive_retention: ArchiveRetentionPolicy,
) -> Result<PackagePreparationProgress, String> {
    let controller = Arc::clone(&state.package_preparation);
    blocking(move || {
        controller.start(
            InstallationId(installation_id),
            destination_access_handle,
            destination_conflict_policy,
            archive_retention,
        )
    })
    .await
}

#[tauri::command]
pub async fn inspect_package_destination(
    state: tauri::State<'_, AppState>,
    installation_id: String,
    destination_access_handle: String,
) -> Result<PackageDestinationPreview, String> {
    let controller = Arc::clone(&state.package_preparation);
    blocking(move || {
        controller.inspect_destination(&InstallationId(installation_id), &destination_access_handle)
    })
    .await
}

#[tauri::command]
pub fn cancel_package_preparation(
    state: tauri::State<'_, AppState>,
    operation_id: String,
) -> Result<bool, String> {
    Ok(state.package_preparation.cancel(&operation_id))
}

#[tauri::command]
pub fn read_package_preparation_progress(
    state: tauri::State<'_, AppState>,
) -> Result<Option<PackagePreparationProgress>, String> {
    Ok(state.package_preparation.latest())
}

#[tauri::command]
pub async fn launch_library_installation(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<LaunchActivity, String> {
    let service = Arc::clone(&state.launch);
    let translation = Arc::clone(&state.translation);
    blocking(move || {
        let installation_id = InstallationId(installation_id);
        translation.before_launch(&installation_id);
        let result = service.launch(LaunchInstallationRequest {
            installation_id,
            activity_id: LaunchActivityId(format!("launch-{}", Uuid::new_v4())),
            attempted_at: dla_sqlite::current_timestamp(),
        });
        match &result {
            Ok(activity) => log::info!(target: "dla::launch", "event=launch_started activity_id={}", activity.id.0),
            Err(error) => log::warn!(target: "dla::launch", "event=launch_failed error={error}"),
        }
        result
    })
    .await
}

#[tauri::command]
pub async fn stop_library_launch(
    state: tauri::State<'_, AppState>,
    activity_id: String,
) -> Result<LaunchActivity, String> {
    let service = Arc::clone(&state.launch);
    blocking(move || {
        let result = service.stop(&LaunchActivityId(activity_id));
        match &result {
            Ok(activity) => log::info!(target: "dla::launch", "event=launch_stopped activity_id={}", activity.id.0),
            Err(error) => log::warn!(target: "dla::launch", "event=launch_stop_failed error={error}"),
        }
        result
    })
        .await
}

#[tauri::command]
pub async fn list_recent_launches(
    state: tauri::State<'_, AppState>,
    limit: u32,
) -> Result<Vec<LaunchActivity>, String> {
    let service = Arc::clone(&state.launch);
    blocking(move || service.recent(limit)).await
}

#[tauri::command]
pub async fn list_installation_launch_history(
    state: tauri::State<'_, AppState>,
    installation_id: String,
    limit: u32,
) -> Result<Vec<LaunchActivity>, String> {
    let service = Arc::clone(&state.launch);
    blocking(move || service.history(&InstallationId(installation_id), limit)).await
}

#[tauri::command]
pub async fn open_library_media_session(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<MediaSession, String> {
    let service = Arc::clone(&state.media);
    blocking(move || {
        service.open(OpenMediaSessionRequest {
            installation_id: InstallationId(installation_id),
            session_id: MediaSessionId(format!("media-{}", Uuid::new_v4())),
            opened_at: dla_sqlite::current_timestamp(),
        })
    })
    .await
}

#[tauri::command]
pub async fn list_library_media_items(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<Vec<MediaSessionItem>, String> {
    let service = Arc::clone(&state.media);
    blocking(move || {
        service.list_items(
            &InstallationId(installation_id),
            &dla_sqlite::current_timestamp(),
        )
    })
    .await
}

#[tauri::command]
pub async fn read_library_audio_waveform(
    state: tauri::State<'_, AppState>,
    installation_id: String,
    ordinal: u32,
    bucket_count: u32,
) -> Result<AudioWaveform, String> {
    let service = Arc::clone(&state.media);
    blocking(move || {
        service.read_audio_waveform(
            &InstallationId(installation_id),
            ordinal,
            bucket_count,
            &dla_sqlite::current_timestamp(),
        )
    })
    .await
}

#[tauri::command]
pub async fn open_personalized_voice_queue(
    state: tauri::State<'_, AppState>,
) -> Result<MediaSession, String> {
    let service = Arc::clone(&state.personalized_voice);
    blocking(move || {
        service.open(OpenPersonalizedVoiceQueueRequest {
            session_id: MediaSessionId(format!("media-{}", Uuid::new_v4())),
            opened_at: dla_sqlite::current_timestamp(),
        })
    })
    .await
}

#[tauri::command]
pub async fn read_library_media_session(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<MediaSession, String> {
    let service = Arc::clone(&state.media);
    blocking(move || service.read(&MediaSessionId(session_id))).await
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaProgressPayload {
    session_id: String,
    item_ordinal: u32,
    position_ms: u64,
    duration_ms: Option<u64>,
    completed: bool,
    status: MediaSessionStatus,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaQueueSettingsPayload {
    session_id: String,
    repeat_mode: MediaRepeatMode,
    shuffle: bool,
}

#[tauri::command]
pub async fn update_library_media_progress(
    state: tauri::State<'_, AppState>,
    request: MediaProgressPayload,
) -> Result<MediaSession, String> {
    let service = Arc::clone(&state.media);
    blocking(move || {
        service.update_progress(UpdateMediaProgressRequest {
            session_id: MediaSessionId(request.session_id),
            item_ordinal: request.item_ordinal,
            position_ms: request.position_ms,
            duration_ms: request.duration_ms,
            completed: request.completed,
            status: request.status,
            updated_at: dla_sqlite::current_timestamp(),
        })
    })
    .await
}

#[tauri::command]
pub async fn update_library_media_queue_settings(
    state: tauri::State<'_, AppState>,
    request: MediaQueueSettingsPayload,
) -> Result<MediaSession, String> {
    let service = Arc::clone(&state.media);
    blocking(move || {
        service.update_queue_settings(UpdateMediaQueueSettingsRequest {
            session_id: MediaSessionId(request.session_id),
            repeat_mode: request.repeat_mode,
            shuffle: request.shuffle,
            updated_at: dla_sqlite::current_timestamp(),
        })
    })
    .await
}

#[tauri::command]
pub async fn close_library_media_session(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<MediaSession, String> {
    state.native_video.close(&app, &session_id)?;
    let service = Arc::clone(&state.media);
    blocking(move || service.close(&MediaSessionId(session_id), dla_sqlite::current_timestamp()))
        .await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn open_native_video(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    request: OpenNativeVideoRequest,
) -> Result<OpenNativeVideoResponse, String> {
    state.native_video.open(&app, request)
}

#[cfg(mobile)]
#[tauri::command]
pub async fn open_native_video(
    _app: AppHandle,
    _state: tauri::State<'_, AppState>,
    _request: OpenNativeVideoRequest,
) -> Result<OpenNativeVideoResponse, String> {
    Err("native video playback is not available on this platform".to_owned())
}

#[cfg(desktop)]
#[tauri::command]
pub fn update_native_video_viewport(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    session_id: String,
    surface_id: String,
    viewport: NativeVideoViewport,
    obstructions: Option<Vec<NativeVideoRectangle>>,
) -> Result<(), String> {
    state.native_video.update_viewport(
        &app,
        &session_id,
        &surface_id,
        viewport,
        obstructions.unwrap_or_default(),
    )
}

#[cfg(mobile)]
#[tauri::command]
pub fn update_native_video_viewport(
    _app: AppHandle,
    _state: tauri::State<'_, AppState>,
    _session_id: String,
    _surface_id: String,
    _viewport: NativeVideoViewport,
    _obstructions: Option<Vec<NativeVideoRectangle>>,
) -> Result<(), String> {
    Err("native video playback is not available on this platform".to_owned())
}

#[cfg(desktop)]
#[tauri::command]
pub fn control_native_video(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    request: NativeVideoControlRequest,
) -> Result<(), String> {
    state.native_video.control(&app, request)
}

#[cfg(mobile)]
#[tauri::command]
pub fn control_native_video(
    _app: AppHandle,
    _state: tauri::State<'_, AppState>,
    _request: NativeVideoControlRequest,
) -> Result<(), String> {
    Err("native video playback is not available on this platform".to_owned())
}

#[cfg(desktop)]
#[tauri::command]
pub fn close_native_video(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    session_id: String,
    surface_id: String,
) -> Result<(), String> {
    state
        .native_video
        .close_surface(&app, &session_id, &surface_id)
}

#[cfg(mobile)]
#[tauri::command]
pub fn close_native_video(
    _app: AppHandle,
    _state: tauri::State<'_, AppState>,
    _session_id: String,
    _surface_id: String,
) -> Result<(), String> {
    Ok(())
}

/// Which picture this build plays video on: its own native one (a webview
/// window, or GStreamer over the page) or the page's `<video>` element. The
/// page asks once at startup and only opens a native surface when told to.
#[tauri::command]
pub fn read_video_backend() -> &'static str {
    if cfg!(any(mobile, native_video_backend = "none")) {
        "page"
    } else {
        "native"
    }
}

/// The sidecar subtitle files beside a video, for the page's own player; a
/// native picture lists them as it opens.
#[tauri::command]
pub async fn read_video_subtitles(
    app: AppHandle,
    session_id: String,
    ordinal: u32,
) -> Result<Vec<dla_media::SidecarSubtitle>, String> {
    blocking(move || {
        crate::media_protocol::video_subtitles(&app, &MediaSessionId(session_id), ordinal)
    })
    .await
}

/// Opens a video of a media session in the system's video player, for files the
/// launcher's own player cannot decode.
#[cfg(desktop)]
#[tauri::command]
pub async fn open_video_externally(
    app: AppHandle,
    session_id: String,
    ordinal: u32,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;

    let path = crate::media_protocol::video_asset_path(&app, &MediaSessionId(session_id), ordinal)?;
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(display_error)
}

#[cfg(mobile)]
#[tauri::command]
pub async fn open_video_externally(_session_id: String, _ordinal: u32) -> Result<(), String> {
    Err("opening videos in another player is available on Windows and Linux".to_owned())
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedCatalogPackage {
    access_handle: String,
    display_name: String,
}

#[cfg(desktop)]
#[tauri::command]
pub async fn select_catalog_package(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Option<SelectedCatalogPackage>, String> {
    let Some(path) = pick_file(
        app,
        "Choose a DLA package",
        Some(("DLA package", CATALOG_PACKAGE_FILE_EXTENSIONS)),
    )
    .await?
    else {
        return Ok(None);
    };
    let approved = state
        .catalog_import
        .approve_package(&path)
        .map_err(display_error)?;
    Ok(Some(SelectedCatalogPackage {
        access_handle: approved.access_handle,
        display_name: approved.display_name,
    }))
}

#[cfg(mobile)]
#[tauri::command]
pub async fn select_catalog_package(
    _app: AppHandle,
    _state: tauri::State<'_, AppState>,
) -> Result<Option<SelectedCatalogPackage>, String> {
    Err("catalog package selection is not available on this platform yet".to_owned())
}

#[tauri::command]
pub async fn inspect_catalog_package(
    state: tauri::State<'_, AppState>,
    access_handle: String,
) -> Result<CatalogImportPreview, String> {
    let controller = Arc::clone(&state.catalog_import);
    blocking(move || controller.inspect(&access_handle)).await
}

#[tauri::command]
pub async fn start_catalog_import(
    state: tauri::State<'_, AppState>,
    access_handle: String,
) -> Result<CatalogImportProgress, String> {
    let controller = Arc::clone(&state.catalog_import);
    blocking(move || controller.start_import(access_handle)).await
}

#[tauri::command]
pub fn cancel_catalog_import(
    state: tauri::State<'_, AppState>,
    operation_id: String,
) -> Result<bool, String> {
    Ok(state.catalog_import.cancel(&operation_id))
}

#[tauri::command]
pub fn read_catalog_import_progress(
    state: tauri::State<'_, AppState>,
) -> Result<Option<CatalogImportProgress>, String> {
    Ok(state.catalog_import.latest())
}

#[tauri::command]
pub async fn list_catalog_generations(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<CatalogGenerationSummary>, String> {
    let controller = Arc::clone(&state.catalog_import);
    blocking(move || controller.list_generations()).await
}

#[tauri::command]
pub async fn activate_catalog_generation(
    state: tauri::State<'_, AppState>,
    generation_id: String,
) -> Result<CatalogImportProgress, String> {
    let controller = Arc::clone(&state.catalog_import);
    blocking(move || controller.start_activation(generation_id)).await
}

#[tauri::command]
pub async fn remove_catalog_generation(
    state: tauri::State<'_, AppState>,
    generation_id: String,
) -> Result<(), String> {
    let controller = Arc::clone(&state.catalog_import);
    blocking(move || controller.remove_generation(&generation_id)).await
}

#[tauri::command]
pub async fn browse_catalog(
    state: tauri::State<'_, AppState>,
    request: BrowseRequest,
) -> Result<CatalogPage, String> {
    let catalog = Arc::clone(&state.catalog);
    blocking(move || catalog.browse(request)).await
}

#[tauri::command]
pub async fn read_catalog_context(
    state: tauri::State<'_, AppState>,
    request: CatalogContextRequest,
) -> Result<CatalogContext, String> {
    let catalog = Arc::clone(&state.catalog);
    blocking(move || catalog.context(request)).await
}

#[tauri::command]
pub async fn read_catalog_work(
    state: tauri::State<'_, AppState>,
    code: String,
) -> Result<CatalogWorkDetail, String> {
    let catalog = Arc::clone(&state.catalog);
    blocking(move || catalog.read(&code)).await
}

#[tauri::command]
pub async fn read_catalog_works(
    state: tauri::State<'_, AppState>,
    codes: Vec<String>,
) -> Result<Vec<CatalogWork>, String> {
    let catalog = Arc::clone(&state.catalog);
    blocking(move || catalog.read_works(&codes)).await
}

#[tauri::command]
pub async fn read_catalog_recommendations(
    state: tauri::State<'_, AppState>,
    code: String,
) -> Result<CatalogRecommendations, String> {
    let recommendations = Arc::clone(&state.recommendations);
    blocking(move || recommendations.read(&code)).await
}

#[tauri::command]
pub async fn read_catalog_rom_contents(
    state: tauri::State<'_, AppState>,
    work_code: String,
    rom_position: usize,
) -> Result<CatalogRomContents, String> {
    let catalog = Arc::clone(&state.catalog);
    blocking(move || catalog.read_rom_contents(&work_code, rom_position)).await
}

#[tauri::command]
pub fn read_search_index_status(state: tauri::State<'_, AppState>) -> SearchIndexStatus {
    state.search.status()
}

#[tauri::command]
pub async fn rebuild_search_index(
    state: tauri::State<'_, AppState>,
) -> Result<SearchRebuildProgress, String> {
    let controller = Arc::clone(&state.search_index_controller);
    blocking(move || controller.start()).await
}

#[tauri::command]
pub fn cancel_search_index_rebuild(
    state: tauri::State<'_, AppState>,
    operation_id: String,
) -> Result<bool, String> {
    Ok(state.search_index_controller.cancel(&operation_id))
}

#[tauri::command]
pub fn read_search_index_rebuild_progress(
    state: tauri::State<'_, AppState>,
) -> Result<Option<SearchRebuildProgress>, String> {
    Ok(state.search_index_controller.latest())
}

#[tauri::command]
pub async fn cleanup_search_index_cache(
    state: tauri::State<'_, AppState>,
) -> Result<SearchCacheCleanupReport, String> {
    let controller = Arc::clone(&state.search_index_controller);
    blocking(move || controller.cleanup()).await
}

#[tauri::command]
pub async fn search_catalog(
    state: tauri::State<'_, AppState>,
    request: SearchRequest,
) -> Result<SearchResponse, String> {
    let search = Arc::clone(&state.search);
    blocking(move || search.search(request)).await
}

#[tauri::command]
pub async fn search_catalog_shortcuts(
    state: tauri::State<'_, AppState>,
    request: SearchShortcutRequest,
) -> Result<Vec<SearchShortcut>, String> {
    let search = Arc::clone(&state.search);
    blocking(move || search.shortcuts(request)).await
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedScanRoot {
    access_handle: String,
    display_path: String,
}

#[cfg(desktop)]
impl From<ApprovedScanRoot> for SelectedScanRoot {
    fn from(approved: ApprovedScanRoot) -> Self {
        Self {
            access_handle: approved.access_handle,
            display_path: approved.display_path,
        }
    }
}

/// Where the library folder pickers start: the default library folder, when
/// one is set and present.
#[cfg(desktop)]
fn preferred_scan_directory(scanner: &ScanController) -> Option<String> {
    scanner
        .read_root_preference()
        .ok()
        .filter(|preference| preference.available)
        .and_then(|preference| preference.display_path)
}

#[tauri::command]
pub async fn read_scan_root_preference(
    state: tauri::State<'_, AppState>,
) -> Result<ScanRootPreference, String> {
    let scanner = Arc::clone(&state.scanner);
    blocking(move || scanner.read_root_preference()).await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn select_scan_root(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Option<SelectedScanRoot>, String> {
    let Some(path) = pick_folder(
        app,
        "Choose a library folder",
        preferred_scan_directory(&state.scanner),
    )
    .await?
    else {
        return Ok(None);
    };
    state
        .scanner
        .approve_root(&path)
        .map(|approved| Some(SelectedScanRoot::from(approved)))
        .map_err(display_error)
}

#[cfg(desktop)]
#[tauri::command]
pub async fn select_preferred_scan_root(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Option<ScanRootPreference>, String> {
    let Some(path) = pick_folder(
        app,
        "Choose the default library folder",
        preferred_scan_directory(&state.scanner),
    )
    .await?
    else {
        return Ok(None);
    };
    state
        .scanner
        .configure_root(&path)
        .map(Some)
        .map_err(display_error)
}

#[cfg(mobile)]
#[tauri::command]
pub async fn select_preferred_scan_root(
    _app: AppHandle,
    _state: tauri::State<'_, AppState>,
) -> Result<Option<ScanRootPreference>, String> {
    Err("default folder selection is not available on this platform".to_owned())
}

#[tauri::command]
pub async fn reset_scan_root_preference(
    state: tauri::State<'_, AppState>,
) -> Result<ScanRootPreference, String> {
    let scanner = Arc::clone(&state.scanner);
    blocking(move || scanner.reset_root_preference()).await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn prepare_preferred_scan_root(
    state: tauri::State<'_, AppState>,
) -> Result<SelectedScanRoot, String> {
    let scanner = Arc::clone(&state.scanner);
    blocking(move || scanner.approve_preferred_root())
        .await
        .map(SelectedScanRoot::from)
}

#[cfg(mobile)]
#[tauri::command]
pub async fn prepare_preferred_scan_root(
    _state: tauri::State<'_, AppState>,
) -> Result<SelectedScanRoot, String> {
    Err("a default scan root is not available on this platform".to_owned())
}

#[cfg(mobile)]
#[tauri::command]
pub async fn select_scan_root(
    _app: AppHandle,
    _state: tauri::State<'_, AppState>,
) -> Result<Option<SelectedScanRoot>, String> {
    Err("desktop folder selection is not available on this platform".to_owned())
}

#[tauri::command]
pub async fn start_library_scan(
    state: tauri::State<'_, AppState>,
    access_handle: String,
) -> Result<ScanSessionView, String> {
    let scanner = Arc::clone(&state.scanner);
    blocking(move || scanner.start(access_handle)).await
}

#[tauri::command]
pub fn cancel_library_scan(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<bool, String> {
    state
        .scanner
        .cancel(&ScanSessionId(session_id))
        .map_err(display_error)
}

#[tauri::command]
pub async fn read_latest_library_scan(
    state: tauri::State<'_, AppState>,
) -> Result<Option<ScanSessionView>, String> {
    let scanner = Arc::clone(&state.scanner);
    blocking(move || scanner.read_latest()).await
}

#[tauri::command]
pub async fn browse_library_scan_issues(
    state: tauri::State<'_, AppState>,
    request: ScanIssueRequest,
) -> Result<ScanIssuePage, String> {
    let scanner = Arc::clone(&state.scanner);
    blocking(move || scanner.browse_issues(request)).await
}

#[tauri::command]
pub async fn list_scan_findings(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<ScanFindingList, String> {
    let service = Arc::clone(&state.scan_findings);
    blocking(move || service.list(&ScanSessionId(session_id))).await
}

#[tauri::command]
pub async fn set_scan_finding_ignored(
    state: tauri::State<'_, AppState>,
    root_id: String,
    key: String,
    ignored: bool,
) -> Result<(), String> {
    let service = Arc::clone(&state.scan_findings);
    blocking(move || {
        service.set_ignored(
            &ScanRootId(root_id),
            &key,
            ignored,
            &dla_sqlite::current_timestamp(),
        )
    })
    .await
}

#[tauri::command]
pub async fn add_scan_findings(
    state: tauri::State<'_, AppState>,
    request: AddScanFindingsRequest,
) -> Result<ScanAddResponse, String> {
    let controller = Arc::clone(&state.scan_installs);
    blocking(move || controller.add(request)).await
}

#[tauri::command]
pub async fn install_library_archive(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<ScanInstallView, String> {
    let controller = Arc::clone(&state.scan_installs);
    blocking(move || controller.install_existing(InstallationId(installation_id))).await
}

#[tauri::command]
pub async fn read_scan_installs(
    state: tauri::State<'_, AppState>,
) -> Result<ScanInstallView, String> {
    let controller = Arc::clone(&state.scan_installs);
    blocking_value(move || controller.view()).await
}

#[tauri::command]
pub async fn cancel_scan_install(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<ScanInstallView, String> {
    let controller = Arc::clone(&state.scan_installs);
    blocking_value(move || controller.cancel(&InstallationId(installation_id))).await
}

/// Opens the picker for "+ Add" and starts finding the picked works; None
/// when the picker is closed without a choice.
#[cfg(desktop)]
#[tauri::command]
pub async fn start_library_add(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    request: dla_application::library_add::StartLibraryAddRequest,
) -> Result<Option<dla_application::library_add::LibraryAddView>, String> {
    use dla_application::library_add::LibraryAddSourceRequest;
    use tauri_plugin_dialog::DialogExt;

    use crate::library_add::LibraryAddPick;

    let controller = Arc::clone(&state.library_add);
    controller.check_can_start()?;
    let pick = blocking(move || {
        let dialog = app.dialog().file();
        let paths = |picked: Option<Vec<tauri_plugin_dialog::FilePath>>| {
            picked
                .map(|picked| {
                    picked
                        .into_iter()
                        .map(|path| path.into_path())
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()
                .map_err(display_error)
        };
        Ok::<_, String>(match request.source {
            LibraryAddSourceRequest::Folders => paths(
                dialog
                    .set_title("Choose work folders to add")
                    .blocking_pick_folders(),
            )?
            .filter(|picked| !picked.is_empty())
            .map(LibraryAddPick::Folders),
            LibraryAddSourceRequest::Archives => paths(
                dialog
                    .set_title("Choose archives to add")
                    .add_filter("Archives", &["zip", "rar", "exe"])
                    .blocking_pick_files(),
            )?
            .filter(|picked| !picked.is_empty())
            .map(LibraryAddPick::Archives),
            LibraryAddSourceRequest::Scan => dialog
                .set_title("Choose a folder to look through for works")
                .blocking_pick_folder()
                .map(|path| path.into_path())
                .transpose()
                .map_err(display_error)?
                .map(LibraryAddPick::Scan),
        })
    })
    .await?;
    let Some(pick) = pick else {
        return Ok(None);
    };
    blocking(move || controller.start(pick).map(Some)).await
}

#[cfg(mobile)]
#[tauri::command]
pub async fn start_library_add(
    _app: AppHandle,
    _request: dla_application::library_add::StartLibraryAddRequest,
) -> Result<Option<dla_application::library_add::LibraryAddView>, String> {
    Err("adding works from folders is not available on this device".to_owned())
}

#[cfg(desktop)]
#[tauri::command]
pub fn read_library_add(
    state: tauri::State<'_, AppState>,
) -> Option<dla_application::library_add::LibraryAddView> {
    state.library_add.view()
}

#[cfg(mobile)]
#[tauri::command]
pub fn read_library_add() -> Option<dla_application::library_add::LibraryAddView> {
    None
}

#[cfg(desktop)]
#[tauri::command]
pub async fn confirm_library_add(
    state: tauri::State<'_, AppState>,
    request: dla_application::library_add::ConfirmLibraryAddRequest,
) -> Result<dla_application::library_add::LibraryAddView, String> {
    let controller = Arc::clone(&state.library_add);
    blocking(move || controller.confirm(request)).await
}

#[cfg(mobile)]
#[tauri::command]
pub async fn confirm_library_add(
    _request: dla_application::library_add::ConfirmLibraryAddRequest,
) -> Result<dla_application::library_add::LibraryAddView, String> {
    Err("adding works from folders is not available on this device".to_owned())
}

#[cfg(desktop)]
#[tauri::command]
pub async fn cancel_library_add(
    state: tauri::State<'_, AppState>,
    batch_id: String,
) -> Result<dla_application::library_add::LibraryAddView, String> {
    let controller = Arc::clone(&state.library_add);
    blocking(move || controller.cancel(&batch_id)).await
}

#[cfg(mobile)]
#[tauri::command]
pub async fn cancel_library_add(
    _batch_id: String,
) -> Result<dla_application::library_add::LibraryAddView, String> {
    Err("adding works from folders is not available on this device".to_owned())
}

#[cfg(desktop)]
#[tauri::command]
pub fn dismiss_library_add(
    state: tauri::State<'_, AppState>,
    batch_id: String,
) -> Result<(), String> {
    state.library_add.dismiss(&batch_id)
}

#[cfg(mobile)]
#[tauri::command]
pub fn dismiss_library_add(_batch_id: String) -> Result<(), String> {
    Err("adding works from folders is not available on this device".to_owned())
}

#[tauri::command]
pub async fn list_library_installations(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Installation>, String> {
    let service = Arc::clone(&state.installation_review);
    blocking(move || service.list()).await
}

#[tauri::command]
pub async fn read_library_shelves(
    state: tauri::State<'_, AppState>,
) -> Result<LibraryShelves, String> {
    let service = Arc::clone(&state.library_shelves);
    blocking(move || service.read()).await
}

#[tauri::command]
pub async fn read_work_preference(
    state: tauri::State<'_, AppState>,
    work_code: String,
) -> Result<Option<WorkPreference>, String> {
    let service = Arc::clone(&state.work_preferences);
    blocking(move || service.read(&work_code)).await
}

#[tauri::command]
pub async fn list_work_preferences(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<WorkPreference>, String> {
    let service = Arc::clone(&state.work_preferences);
    blocking(move || service.list()).await
}

#[tauri::command]
pub async fn replace_work_preference(
    state: tauri::State<'_, AppState>,
    work_code: String,
    preference: Option<WorkPreferenceKind>,
) -> Result<Option<WorkPreference>, String> {
    let service = Arc::clone(&state.work_preferences);
    blocking(move || service.replace(&work_code, preference, &dla_sqlite::current_timestamp()))
        .await
}

#[tauri::command]
pub async fn read_local_personalization(
    state: tauri::State<'_, AppState>,
) -> Result<LocalPersonalization, String> {
    let service = Arc::clone(&state.local_personalization);
    blocking(move || service.read()).await
}

#[tauri::command]
pub async fn list_library_installations_for_work(
    state: tauri::State<'_, AppState>,
    work_code: String,
) -> Result<Vec<Installation>, String> {
    let service = Arc::clone(&state.installation_review);
    blocking(move || service.list_for_work(&work_code)).await
}

#[tauri::command]
pub async fn read_library_installation(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<Installation, String> {
    let service = Arc::clone(&state.installation_review);
    blocking(move || service.read(&InstallationId(installation_id))).await
}

#[tauri::command]
pub async fn save_library_installation_review(
    state: tauri::State<'_, AppState>,
    request: InstallationReviewRequest,
) -> Result<Installation, String> {
    let service = Arc::clone(&state.installation_review);
    blocking(move || service.save(request, dla_sqlite::current_timestamp())).await
}

#[tauri::command]
pub async fn list_own_work_details(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<OwnWorkDetails>, String> {
    let service = Arc::clone(&state.own_works);
    blocking(move || service.list()).await
}

#[tauri::command]
pub async fn read_own_work_details(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<Option<OwnWorkDetails>, String> {
    let service = Arc::clone(&state.own_works);
    blocking(move || service.read(&InstallationId(installation_id))).await
}

#[tauri::command]
pub async fn save_own_work_details(
    state: tauri::State<'_, AppState>,
    request: SaveOwnWorkDetailsRequest,
) -> Result<OwnWorkDetails, String> {
    let service = Arc::clone(&state.own_works);
    blocking(move || service.save(request, &dla_sqlite::current_timestamp())).await
}

#[tauri::command]
pub async fn list_installation_images(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<Vec<InstallationImage>, String> {
    let service = Arc::clone(&state.own_works);
    blocking(move || service.list_images(&InstallationId(installation_id))).await
}

#[tauri::command]
pub async fn import_own_work_artwork(
    state: tauri::State<'_, AppState>,
    request: ImportOwnWorkArtworkRequest,
) -> Result<String, String> {
    let service = Arc::clone(&state.own_works);
    blocking(move || service.import_artwork(request)).await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn import_own_work_artwork_file(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<Option<String>, String> {
    let Some(path) = pick_file(
        app,
        "Choose a picture",
        Some(("Pictures", OWN_WORK_PICTURE_EXTENSIONS)),
    )
    .await?
    else {
        return Ok(None);
    };
    let service = Arc::clone(&state.own_works);
    blocking(move || service.import_artwork_file(&InstallationId(installation_id), &path))
        .await
        .map(Some)
}

#[cfg(desktop)]
const OWN_WORK_PICTURE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "bmp"];

#[cfg(mobile)]
#[tauri::command]
pub async fn import_own_work_artwork_file(
    _app: AppHandle,
    _state: tauri::State<'_, AppState>,
    _installation_id: String,
) -> Result<Option<String>, String> {
    Err("choosing a picture file is not available on this platform yet".to_owned())
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowSize {
    width: u32,
    height: u32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowMetrics {
    width: u32,
    height: u32,
    work_area_width: u32,
    work_area_height: u32,
    scale_factor: f64,
    maximized: bool,
    supports_window_controls: bool,
}

#[tauri::command]
pub async fn read_system_report() -> Result<crate::system_report::SystemReport, String> {
    blocking_value(crate::system_report::read_system_report).await
}

#[tauri::command]
pub fn read_window_metrics(window: WebviewWindow) -> Result<WindowMetrics, String> {
    window_metrics(&window)
}

#[cfg(desktop)]
#[tauri::command]
pub fn resize_window(window: WebviewWindow, size: WindowSize) -> Result<WindowMetrics, String> {
    let work_area = current_work_area(&window)?;
    let width = size.width.clamp(360, work_area.width.max(360));
    let height = size.height.clamp(560, work_area.height.max(560));
    if window.is_maximized().map_err(display_error)? {
        window.unmaximize().map_err(display_error)?;
    }
    window
        .set_size(LogicalSize::new(width, height))
        .map_err(display_error)?;
    window.center().map_err(display_error)?;
    window_metrics(&window)
}

#[cfg(mobile)]
#[tauri::command]
pub fn resize_window(_window: WebviewWindow, size: WindowSize) -> Result<WindowMetrics, String> {
    Err(format!(
        "native window resizing to {} × {} is available only on desktop",
        size.width, size.height
    ))
}

#[cfg(desktop)]
#[tauri::command]
pub fn maximize_window(window: WebviewWindow) -> Result<WindowMetrics, String> {
    window.maximize().map_err(display_error)?;
    window_metrics(&window)
}

#[cfg(mobile)]
#[tauri::command]
pub fn maximize_window(_window: WebviewWindow) -> Result<WindowMetrics, String> {
    Err("native window maximization is available only on desktop".to_owned())
}

fn window_metrics(window: &WebviewWindow) -> Result<WindowMetrics, String> {
    let scale_factor = window.scale_factor().map_err(display_error)?;
    let size = window
        .inner_size()
        .map_err(display_error)?
        .to_logical::<u32>(scale_factor);
    let work_area = current_work_area(window)?;
    Ok(WindowMetrics {
        width: size.width,
        height: size.height,
        work_area_width: work_area.width,
        work_area_height: work_area.height,
        scale_factor,
        maximized: window.is_maximized().map_err(display_error)?,
        supports_window_controls: cfg!(desktop),
    })
}

fn current_work_area(window: &WebviewWindow) -> Result<LogicalSize<u32>, String> {
    let monitor = window
        .current_monitor()
        .map_err(display_error)?
        .ok_or_else(|| "the native window is not associated with a monitor".to_owned())?;
    let scale_factor = monitor.scale_factor();
    Ok(monitor.work_area().size.to_logical(scale_factor))
}

fn display_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(desktop)]
use crate::delivery::{DeliveryFailure, DeliveryView};

#[cfg(desktop)]
async fn delivery_call(
    state: tauri::State<'_, AppState>,
    call: impl FnOnce(
        &Arc<crate::delivery::DeliveryController>,
    ) -> Result<DeliveryView, dla_application::delivery::DeliveryError>
    + Send
    + 'static,
) -> Result<DeliveryView, DeliveryFailure> {
    let controller = Arc::clone(&state.delivery);
    blocking_value(move || call(&controller).map_err(DeliveryFailure::from))
        .await
        .map_err(|detail| DeliveryFailure {
            problem: dla_domain::delivery::DeliveryProblem::Unavailable,
            detail,
        })?
}

#[cfg(desktop)]
fn delivery_job(value: String) -> dla_domain::delivery::DeliveryJobId {
    dla_domain::delivery::DeliveryJobId(value)
}

#[cfg(desktop)]
#[tauri::command]
pub async fn read_delivery(
    state: tauri::State<'_, AppState>,
) -> Result<DeliveryView, DeliveryFailure> {
    delivery_call(state, |delivery| delivery.view()).await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn delivery_sign_in(
    state: tauri::State<'_, AppState>,
    username: String,
    password: String,
) -> Result<DeliveryView, DeliveryFailure> {
    let password = zeroize::Zeroizing::new(password);
    delivery_call(state, move |delivery| delivery.sign_in(&username, password)).await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn delivery_sign_out(
    state: tauri::State<'_, AppState>,
) -> Result<DeliveryView, DeliveryFailure> {
    delivery_call(state, |delivery| delivery.sign_out()).await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn delivery_install(
    state: tauri::State<'_, AppState>,
    work_code: String,
) -> Result<DeliveryView, DeliveryFailure> {
    delivery_call(state, move |delivery| delivery.install(&work_code)).await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn delivery_pause(
    state: tauri::State<'_, AppState>,
    job_id: String,
) -> Result<DeliveryView, DeliveryFailure> {
    delivery_call(state, move |delivery| delivery.pause(&delivery_job(job_id))).await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn delivery_resume(
    state: tauri::State<'_, AppState>,
    job_id: String,
) -> Result<DeliveryView, DeliveryFailure> {
    delivery_call(state, move |delivery| {
        delivery.resume(&delivery_job(job_id))
    })
    .await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn delivery_cancel(
    state: tauri::State<'_, AppState>,
    job_id: String,
) -> Result<DeliveryView, DeliveryFailure> {
    delivery_call(state, move |delivery| {
        delivery.cancel(&delivery_job(job_id))
    })
    .await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn delivery_remove(
    state: tauri::State<'_, AppState>,
    job_id: String,
) -> Result<DeliveryView, DeliveryFailure> {
    delivery_call(state, move |delivery| {
        delivery.remove(&delivery_job(job_id))
    })
    .await
}

/// Opens the folder picker for the install folder that Install from DLA and
/// the scanner share. `None` means the picker was closed.
#[cfg(desktop)]
async fn pick_install_folder(app: AppHandle) -> Result<Option<std::path::PathBuf>, String> {
    pick_folder(
        app,
        "Choose where works will be installed",
        default_install_root(),
    )
    .await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn select_delivery_install_folder(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Option<DeliveryView>, DeliveryFailure> {
    let Some(path) = pick_install_folder(app)
        .await
        .map_err(|detail| DeliveryFailure {
            problem: dla_domain::delivery::DeliveryProblem::InstallFolder,
            detail,
        })?
    else {
        return Ok(None);
    };
    let installs = Arc::clone(&state.scan_installs);
    let view = delivery_call(state, move |delivery| delivery.set_install_folder(&path)).await?;
    installs.changed();
    Ok(Some(view))
}

#[cfg(desktop)]
#[tauri::command]
pub async fn select_install_folder(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Option<ScanInstallView>, String> {
    let Some(path) = pick_install_folder(app).await? else {
        return Ok(None);
    };
    let delivery = Arc::clone(&state.delivery);
    let installs = Arc::clone(&state.scan_installs);
    blocking(move || {
        delivery.set_install_folder(&path).map_err(display_error)?;
        installs.changed();
        Ok::<_, String>(Some(installs.view()))
    })
    .await
}

#[cfg(mobile)]
#[tauri::command]
pub async fn select_install_folder() -> Result<(), String> {
    Err("installing archives is available on Windows and Linux".to_owned())
}

#[cfg(mobile)]
const DELIVERY_MOBILE: &str = "Install from DLA is available on Windows and Linux";

#[cfg(mobile)]
#[tauri::command]
pub async fn read_delivery() -> Result<(), String> {
    Err(DELIVERY_MOBILE.to_owned())
}

#[cfg(mobile)]
#[tauri::command]
pub async fn delivery_sign_in(_username: String, _password: String) -> Result<(), String> {
    Err(DELIVERY_MOBILE.to_owned())
}

#[cfg(mobile)]
#[tauri::command]
pub async fn delivery_sign_out() -> Result<(), String> {
    Err(DELIVERY_MOBILE.to_owned())
}

#[cfg(mobile)]
#[tauri::command]
pub async fn delivery_install(_work_code: String) -> Result<(), String> {
    Err(DELIVERY_MOBILE.to_owned())
}

#[cfg(mobile)]
#[tauri::command]
pub async fn delivery_pause(_job_id: String) -> Result<(), String> {
    Err(DELIVERY_MOBILE.to_owned())
}

#[cfg(mobile)]
#[tauri::command]
pub async fn delivery_resume(_job_id: String) -> Result<(), String> {
    Err(DELIVERY_MOBILE.to_owned())
}

#[cfg(mobile)]
#[tauri::command]
pub async fn delivery_cancel(_job_id: String) -> Result<(), String> {
    Err(DELIVERY_MOBILE.to_owned())
}

#[cfg(mobile)]
#[tauri::command]
pub async fn delivery_remove(_job_id: String) -> Result<(), String> {
    Err(DELIVERY_MOBILE.to_owned())
}

#[cfg(mobile)]
#[tauri::command]
pub async fn select_delivery_install_folder() -> Result<(), String> {
    Err(DELIVERY_MOBILE.to_owned())
}
