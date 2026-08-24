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
    diagnostics::{DiagnosticsService, ProbeReport},
    installation_from_scan::{CreateInstallationFromScanRequest, InstallationFromScanService},
    installation_review::{InstallationReviewRequest, InstallationReviewService},
    launch::{LaunchInstallationRequest, LaunchService},
    library_shelves::LibraryShelvesService,
    maintenance::LibraryMaintenanceService,
    media::{
        AudioWaveform, MediaService, OpenMediaSessionRequest, OpenPersonalizedVoiceQueueRequest,
        PersonalizedVoiceQueueService, UpdateMediaProgressRequest, UpdateMediaQueueSettingsRequest,
    },
    package_preparation::{PackageDestinationConflictPolicy, PackageDestinationPreview},
    personalization::{LocalPersonalizationService, WorkPreferenceService},
    scanner::{
        ScanIssuePage, ScanIssueRequest, ScanResultPage, ScanResultRequest, ScanRootPreference,
        ScanSessionView,
    },
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
    launch::{LaunchActivity, LaunchActivityId},
    library::{LibraryShelves, LocalPersonalization, WorkPreference, WorkPreferenceKind},
    maintenance::{InstallationHealthReport, MaintenanceCleanupReport},
    media::{MediaRepeatMode, MediaSession, MediaSessionId, MediaSessionItem, MediaSessionStatus},
    package::{ArchiveRetentionPolicy, PackagePreparationProgress, PreparedPackageInstallation},
    scanner::{ScanResultId, ScanSessionId},
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, LogicalSize, WebviewWindow};
use uuid::Uuid;

use crate::{
    catalog_import::CatalogImportController,
    native_video::{
        NativeVideoControlRequest, NativeVideoController, NativeVideoViewport,
        OpenNativeVideoRequest, OpenNativeVideoResponse,
    },
    package_preparation::PackagePreparationController,
    scanner::ScanController,
    search::SearchIndexController,
};

pub struct AppState {
    pub android_app: Arc<AndroidAppService>,
    pub android_package: Arc<AndroidPackageService>,
    pub catalog: Arc<CatalogService>,
    pub cover_cache: Arc<dyn CatalogArtworkCache>,
    pub recommendations: Arc<CatalogRecommendationService>,
    pub diagnostics: Arc<DiagnosticsService>,
    pub search: Arc<CatalogSearchService>,
    pub search_index_controller: Arc<SearchIndexController>,
    pub scanner: Arc<ScanController>,
    pub catalog_import: Arc<CatalogImportController>,
    pub installation_from_scan: Arc<InstallationFromScanService>,
    pub installation_review: Arc<InstallationReviewService>,
    pub library_shelves: Arc<LibraryShelvesService>,
    pub work_preferences: Arc<WorkPreferenceService>,
    pub local_personalization: Arc<LocalPersonalizationService>,
    pub package_preparation: Arc<PackagePreparationController>,
    pub maintenance: Arc<LibraryMaintenanceService>,
    pub launch: Arc<LaunchService>,
    pub media: Arc<MediaService>,
    pub personalized_voice: Arc<PersonalizedVoiceQueueService>,
    pub native_video: Arc<NativeVideoController>,
}

#[tauri::command]
pub async fn list_android_apps(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<AndroidAppView>, String> {
    let service = Arc::clone(&state.android_app);
    tauri::async_runtime::spawn_blocking(move || service.list())
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn associate_installed_android_app(
    state: tauri::State<'_, AppState>,
    work_code: String,
) -> Result<AndroidAppView, String> {
    let app_service = Arc::clone(&state.android_app);
    let package_service = Arc::clone(&state.android_package);
    tauri::async_runtime::spawn_blocking(move || {
        let package_state = package_service
            .read_state()
            .map_err(|error| error.to_string())?;
        app_service
            .associate_installed(
                &work_code,
                AndroidAppAssociationId(format!("android-app-{}", Uuid::new_v4())),
                &dla_sqlite::current_timestamp(),
                &package_state,
            )
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn launch_android_app(
    state: tauri::State<'_, AppState>,
    association_id: String,
) -> Result<AndroidAppView, String> {
    let service = Arc::clone(&state.android_app);
    tauri::async_runtime::spawn_blocking(move || {
        service.launch(
            &AndroidAppAssociationId(association_id),
            &dla_sqlite::current_timestamp(),
        )
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn remove_android_app_association(
    state: tauri::State<'_, AppState>,
    association_id: String,
) -> Result<(), String> {
    let service = Arc::clone(&state.android_app);
    tauri::async_runtime::spawn_blocking(move || {
        service.remove(&AndroidAppAssociationId(association_id))
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn read_android_package_state(
    state: tauri::State<'_, AppState>,
) -> Result<dla_domain::android_package::AndroidPackageState, String> {
    let service = Arc::clone(&state.android_package);
    tauri::async_runtime::spawn_blocking(move || service.read_state())
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn select_and_inspect_android_package(
    state: tauri::State<'_, AppState>,
) -> Result<dla_domain::android_package::AndroidPackageState, String> {
    let service = Arc::clone(&state.android_package);
    tauri::async_runtime::spawn_blocking(move || service.select_and_inspect())
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn clear_android_package_selection(
    state: tauri::State<'_, AppState>,
) -> Result<dla_domain::android_package::AndroidPackageState, String> {
    let service = Arc::clone(&state.android_package);
    tauri::async_runtime::spawn_blocking(move || service.clear_selection())
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn open_android_package_source_approval(
    state: tauri::State<'_, AppState>,
) -> Result<dla_domain::android_package::AndroidPackageState, String> {
    let service = Arc::clone(&state.android_package);
    tauri::async_runtime::spawn_blocking(move || service.open_source_approval())
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn request_android_package_install(
    state: tauri::State<'_, AppState>,
) -> Result<dla_domain::android_package::AndroidPackageState, String> {
    let service = Arc::clone(&state.android_package);
    tauri::async_runtime::spawn_blocking(move || service.request_install())
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn read_cover_cache_summary(
    state: tauri::State<'_, AppState>,
) -> Result<CatalogArtworkCacheSummary, String> {
    state
        .cover_cache
        .summary()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn configure_cover_cache(
    state: tauri::State<'_, AppState>,
    retention: CatalogArtworkRetention,
    capacity: CatalogArtworkCapacity,
) -> Result<CatalogArtworkCacheSummary, String> {
    let cache = Arc::clone(&state.cover_cache);
    tauri::async_runtime::spawn_blocking(move || cache.configure(retention, capacity))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
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
    use tauri_plugin_dialog::DialogExt;

    let handle = app.clone();
    let initial_directory = std::env::var("DLA_DEFAULT_INSTALL_ROOT")
        .ok()
        .filter(|path| std::path::Path::new(path).is_dir());
    let selected = tauri::async_runtime::spawn_blocking(move || {
        let dialog = handle
            .dialog()
            .file()
            .set_title("Choose where this work will be installed");
        match initial_directory {
            Some(directory) => dialog.set_directory(directory).blocking_pick_folder(),
            None => dialog.blocking_pick_folder(),
        }
    })
    .await
    .map_err(|error| error.to_string())?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    let path = selected.into_path().map_err(|error| error.to_string())?;
    let approved = state
        .package_preparation
        .approve_destination(&path)
        .map_err(|error| error.to_string())?;
    Ok(Some(SelectedInstallationDestination {
        access_handle: approved.access_handle,
        display_path: approved.display_path,
    }))
}

#[cfg(desktop)]
#[tauri::command]
pub async fn select_installation_location(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Option<SelectedInstallationDestination>, String> {
    use tauri_plugin_dialog::DialogExt;

    let selected = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Locate the existing installation folder")
            .blocking_pick_folder()
    })
    .await
    .map_err(|error| error.to_string())?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    let path = selected.into_path().map_err(|error| error.to_string())?;
    let approved = state
        .package_preparation
        .approve_destination(&path)
        .map_err(|error| error.to_string())?;
    Ok(Some(SelectedInstallationDestination {
        access_handle: approved.access_handle,
        display_path: approved.display_path,
    }))
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
    tauri::async_runtime::spawn_blocking(move || {
        service.read_health(&InstallationId(installation_id))
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn read_installation_healths(
    state: tauri::State<'_, AppState>,
    installation_ids: Vec<String>,
) -> Result<Vec<InstallationHealthReport>, String> {
    let service = Arc::clone(&state.maintenance);
    tauri::async_runtime::spawn_blocking(move || {
        let installation_ids = installation_ids
            .into_iter()
            .map(InstallationId)
            .collect::<Vec<_>>();
        service.read_healths(&installation_ids)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn verify_library_installation(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<InstallationHealthReport, String> {
    let installation_id = InstallationId(installation_id);
    ensure_not_preparing(&state.package_preparation, &installation_id)?;
    let service = Arc::clone(&state.maintenance);
    tauri::async_runtime::spawn_blocking(move || {
        service.verify(&installation_id, dla_sqlite::current_timestamp())
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
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
        .map_err(|error| error.to_string())?;
    let service = Arc::clone(&state.maintenance);
    tauri::async_runtime::spawn_blocking(move || {
        service.relocate(
            &installation_id,
            selected_root,
            dla_sqlite::current_timestamp(),
        )
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn rescan_library_installation(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<InstallationHealthReport, String> {
    let installation_id = InstallationId(installation_id);
    ensure_not_preparing(&state.package_preparation, &installation_id)?;
    let service = Arc::clone(&state.maintenance);
    tauri::async_runtime::spawn_blocking(move || {
        service.rescan(&installation_id, dla_sqlite::current_timestamp())
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn repair_library_installation(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<InstallationHealthReport, String> {
    let installation_id = InstallationId(installation_id);
    ensure_not_preparing(&state.package_preparation, &installation_id)?;
    let service = Arc::clone(&state.maintenance);
    tauri::async_runtime::spawn_blocking(move || {
        service.repair(
            &installation_id,
            Uuid::new_v4().to_string(),
            dla_sqlite::current_timestamp(),
        )
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn remove_library_installation(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<(), String> {
    let installation_id = InstallationId(installation_id);
    ensure_not_preparing(&state.package_preparation, &installation_id)?;
    let service = Arc::clone(&state.maintenance);
    tauri::async_runtime::spawn_blocking(move || service.remove_from_library(&installation_id))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn uninstall_library_installation(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<(), String> {
    let installation_id = InstallationId(installation_id);
    ensure_not_preparing(&state.package_preparation, &installation_id)?;
    let service = Arc::clone(&state.maintenance);
    tauri::async_runtime::spawn_blocking(move || service.uninstall(&installation_id))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn cleanup_library_maintenance(
    state: tauri::State<'_, AppState>,
) -> Result<MaintenanceCleanupReport, String> {
    if state
        .package_preparation
        .has_active_operation()
        .map_err(|error| error.to_string())?
    {
        return Err("package preparation is currently active".to_owned());
    }
    let service = Arc::clone(&state.maintenance);
    tauri::async_runtime::spawn_blocking(move || service.cleanup_abandoned())
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

fn ensure_not_preparing(
    controller: &PackagePreparationController,
    installation_id: &InstallationId,
) -> Result<(), String> {
    if controller
        .installation_is_active(installation_id)
        .map_err(|error| error.to_string())?
    {
        Err("package preparation is currently active for this installation".to_owned())
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
    tauri::async_runtime::spawn_blocking(move || {
        service.read_prepared_package(&InstallationId(installation_id))
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn read_prepared_packages(
    state: tauri::State<'_, AppState>,
    installation_ids: Vec<String>,
) -> Result<Vec<PreparedPackageInstallation>, String> {
    let service = Arc::clone(&state.media);
    tauri::async_runtime::spawn_blocking(move || {
        let installation_ids = installation_ids
            .into_iter()
            .map(InstallationId)
            .collect::<Vec<_>>();
        service.read_prepared_packages(&installation_ids)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
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
    tauri::async_runtime::spawn_blocking(move || {
        controller.start(
            InstallationId(installation_id),
            destination_access_handle,
            destination_conflict_policy,
            archive_retention,
        )
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn inspect_package_destination(
    state: tauri::State<'_, AppState>,
    installation_id: String,
    destination_access_handle: String,
) -> Result<PackageDestinationPreview, String> {
    let controller = Arc::clone(&state.package_preparation);
    tauri::async_runtime::spawn_blocking(move || {
        controller.inspect_destination(&InstallationId(installation_id), &destination_access_handle)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn cancel_package_preparation(
    state: tauri::State<'_, AppState>,
    operation_id: String,
) -> Result<bool, String> {
    state
        .package_preparation
        .cancel(&operation_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn read_package_preparation_progress(
    state: tauri::State<'_, AppState>,
) -> Result<Option<PackagePreparationProgress>, String> {
    state
        .package_preparation
        .latest()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn launch_library_installation(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<LaunchActivity, String> {
    let service = Arc::clone(&state.launch);
    tauri::async_runtime::spawn_blocking(move || {
        let result = service.launch(LaunchInstallationRequest {
            installation_id: InstallationId(installation_id),
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
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn stop_library_launch(
    state: tauri::State<'_, AppState>,
    activity_id: String,
) -> Result<LaunchActivity, String> {
    let service = Arc::clone(&state.launch);
    tauri::async_runtime::spawn_blocking(move || {
        let result = service.stop(&LaunchActivityId(activity_id));
        match &result {
            Ok(activity) => log::info!(target: "dla::launch", "event=launch_stopped activity_id={}", activity.id.0),
            Err(error) => log::warn!(target: "dla::launch", "event=launch_stop_failed error={error}"),
        }
        result
    })
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn list_recent_launches(
    state: tauri::State<'_, AppState>,
    limit: u32,
) -> Result<Vec<LaunchActivity>, String> {
    let service = Arc::clone(&state.launch);
    tauri::async_runtime::spawn_blocking(move || service.recent(limit))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn list_installation_launch_history(
    state: tauri::State<'_, AppState>,
    installation_id: String,
    limit: u32,
) -> Result<Vec<LaunchActivity>, String> {
    let service = Arc::clone(&state.launch);
    tauri::async_runtime::spawn_blocking(move || {
        service.history(&InstallationId(installation_id), limit)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn open_library_media_session(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<MediaSession, String> {
    let service = Arc::clone(&state.media);
    tauri::async_runtime::spawn_blocking(move || {
        service.open(OpenMediaSessionRequest {
            installation_id: InstallationId(installation_id),
            session_id: MediaSessionId(format!("media-{}", Uuid::new_v4())),
            opened_at: dla_sqlite::current_timestamp(),
        })
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn list_library_media_items(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<Vec<MediaSessionItem>, String> {
    let service = Arc::clone(&state.media);
    tauri::async_runtime::spawn_blocking(move || {
        service.list_items(
            &InstallationId(installation_id),
            &dla_sqlite::current_timestamp(),
        )
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn read_library_audio_waveform(
    state: tauri::State<'_, AppState>,
    installation_id: String,
    ordinal: u32,
    bucket_count: u32,
) -> Result<AudioWaveform, String> {
    let service = Arc::clone(&state.media);
    tauri::async_runtime::spawn_blocking(move || {
        service.read_audio_waveform(
            &InstallationId(installation_id),
            ordinal,
            bucket_count,
            &dla_sqlite::current_timestamp(),
        )
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn open_personalized_voice_queue(
    state: tauri::State<'_, AppState>,
) -> Result<MediaSession, String> {
    let service = Arc::clone(&state.personalized_voice);
    tauri::async_runtime::spawn_blocking(move || {
        service.open(OpenPersonalizedVoiceQueueRequest {
            session_id: MediaSessionId(format!("media-{}", Uuid::new_v4())),
            opened_at: dla_sqlite::current_timestamp(),
        })
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn read_library_media_session(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<MediaSession, String> {
    let service = Arc::clone(&state.media);
    tauri::async_runtime::spawn_blocking(move || service.read(&MediaSessionId(session_id)))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
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
    tauri::async_runtime::spawn_blocking(move || {
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
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn update_library_media_queue_settings(
    state: tauri::State<'_, AppState>,
    request: MediaQueueSettingsPayload,
) -> Result<MediaSession, String> {
    let service = Arc::clone(&state.media);
    tauri::async_runtime::spawn_blocking(move || {
        service.update_queue_settings(UpdateMediaQueueSettingsRequest {
            session_id: MediaSessionId(request.session_id),
            repeat_mode: request.repeat_mode,
            shuffle: request.shuffle,
            updated_at: dla_sqlite::current_timestamp(),
        })
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn close_library_media_session(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<MediaSession, String> {
    state.native_video.close(&app, &session_id)?;
    let service = Arc::clone(&state.media);
    tauri::async_runtime::spawn_blocking(move || {
        service.close(&MediaSessionId(session_id), dla_sqlite::current_timestamp())
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
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
) -> Result<(), String> {
    state
        .native_video
        .update_viewport(&app, &session_id, &surface_id, viewport)
}

#[cfg(mobile)]
#[tauri::command]
pub fn update_native_video_viewport(
    _app: AppHandle,
    _state: tauri::State<'_, AppState>,
    _session_id: String,
    _surface_id: String,
    _viewport: NativeVideoViewport,
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
    use tauri_plugin_dialog::DialogExt;

    let handle = app.clone();
    let selected = tauri::async_runtime::spawn_blocking(move || {
        handle
            .dialog()
            .file()
            .set_title("Choose a DLA package")
            .add_filter("DLA package", CATALOG_PACKAGE_FILE_EXTENSIONS)
            .blocking_pick_file()
    })
    .await
    .map_err(|error| error.to_string())?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    let path = selected.into_path().map_err(|error| error.to_string())?;
    let approved = state
        .catalog_import
        .approve_package(&path)
        .map_err(|error| error.to_string())?;
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
    tauri::async_runtime::spawn_blocking(move || controller.inspect(&access_handle))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn start_catalog_import(
    state: tauri::State<'_, AppState>,
    access_handle: String,
) -> Result<CatalogImportProgress, String> {
    let controller = Arc::clone(&state.catalog_import);
    tauri::async_runtime::spawn_blocking(move || controller.start_import(access_handle))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn cancel_catalog_import(
    state: tauri::State<'_, AppState>,
    operation_id: String,
) -> Result<bool, String> {
    state
        .catalog_import
        .cancel(&operation_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn read_catalog_import_progress(
    state: tauri::State<'_, AppState>,
) -> Result<Option<CatalogImportProgress>, String> {
    state
        .catalog_import
        .latest()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn list_catalog_generations(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<CatalogGenerationSummary>, String> {
    let controller = Arc::clone(&state.catalog_import);
    tauri::async_runtime::spawn_blocking(move || controller.list_generations())
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn activate_catalog_generation(
    state: tauri::State<'_, AppState>,
    generation_id: String,
) -> Result<CatalogImportProgress, String> {
    let controller = Arc::clone(&state.catalog_import);
    tauri::async_runtime::spawn_blocking(move || controller.start_activation(generation_id))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn remove_catalog_generation(
    state: tauri::State<'_, AppState>,
    generation_id: String,
) -> Result<(), String> {
    let controller = Arc::clone(&state.catalog_import);
    tauri::async_runtime::spawn_blocking(move || controller.remove_generation(&generation_id))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn browse_catalog(
    state: tauri::State<'_, AppState>,
    request: BrowseRequest,
) -> Result<CatalogPage, String> {
    let catalog = Arc::clone(&state.catalog);
    tauri::async_runtime::spawn_blocking(move || catalog.browse(request))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn read_catalog_context(
    state: tauri::State<'_, AppState>,
    request: CatalogContextRequest,
) -> Result<CatalogContext, String> {
    let catalog = Arc::clone(&state.catalog);
    tauri::async_runtime::spawn_blocking(move || catalog.context(request))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn read_catalog_work(
    state: tauri::State<'_, AppState>,
    code: String,
) -> Result<CatalogWorkDetail, String> {
    let catalog = Arc::clone(&state.catalog);
    tauri::async_runtime::spawn_blocking(move || catalog.read(&code))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn read_catalog_works(
    state: tauri::State<'_, AppState>,
    codes: Vec<String>,
) -> Result<Vec<CatalogWork>, String> {
    let catalog = Arc::clone(&state.catalog);
    tauri::async_runtime::spawn_blocking(move || catalog.read_works(&codes))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn read_catalog_recommendations(
    state: tauri::State<'_, AppState>,
    code: String,
) -> Result<CatalogRecommendations, String> {
    let recommendations = Arc::clone(&state.recommendations);
    tauri::async_runtime::spawn_blocking(move || recommendations.read(&code))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn read_catalog_rom_contents(
    state: tauri::State<'_, AppState>,
    work_code: String,
    rom_position: usize,
) -> Result<CatalogRomContents, String> {
    let catalog = Arc::clone(&state.catalog);
    tauri::async_runtime::spawn_blocking(move || {
        catalog.read_rom_contents(&work_code, rom_position)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn run_sqlite_probe(state: tauri::State<'_, AppState>) -> Result<ProbeReport, String> {
    let diagnostics = Arc::clone(&state.diagnostics);
    tauri::async_runtime::spawn_blocking(move || diagnostics.run_sqlite_probe())
        .await
        .map_err(|error| error.to_string())
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
    tauri::async_runtime::spawn_blocking(move || controller.start())
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn cancel_search_index_rebuild(
    state: tauri::State<'_, AppState>,
    operation_id: String,
) -> Result<bool, String> {
    state
        .search_index_controller
        .cancel(&operation_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn read_search_index_rebuild_progress(
    state: tauri::State<'_, AppState>,
) -> Result<Option<SearchRebuildProgress>, String> {
    state
        .search_index_controller
        .latest()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn cleanup_search_index_cache(
    state: tauri::State<'_, AppState>,
) -> Result<SearchCacheCleanupReport, String> {
    let controller = Arc::clone(&state.search_index_controller);
    tauri::async_runtime::spawn_blocking(move || controller.cleanup())
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn search_catalog(
    state: tauri::State<'_, AppState>,
    request: SearchRequest,
) -> Result<SearchResponse, String> {
    let search = Arc::clone(&state.search);
    tauri::async_runtime::spawn_blocking(move || search.search(request))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn search_catalog_shortcuts(
    state: tauri::State<'_, AppState>,
    request: SearchShortcutRequest,
) -> Result<Vec<SearchShortcut>, String> {
    let search = Arc::clone(&state.search);
    tauri::async_runtime::spawn_blocking(move || search.shortcuts(request))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedScanRoot {
    access_handle: String,
    display_path: String,
}

#[tauri::command]
pub fn read_scan_root_preference(
    state: tauri::State<'_, AppState>,
) -> Result<ScanRootPreference, String> {
    state
        .scanner
        .read_root_preference()
        .map_err(|error| error.to_string())
}

#[cfg(desktop)]
#[tauri::command]
pub async fn select_scan_root(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Option<SelectedScanRoot>, String> {
    use tauri_plugin_dialog::DialogExt;

    let handle = app.clone();
    let initial_directory = state
        .scanner
        .read_root_preference()
        .ok()
        .filter(|preference| preference.available)
        .and_then(|preference| preference.display_path);
    let selected = tauri::async_runtime::spawn_blocking(move || {
        let dialog = handle.dialog().file().set_title("Choose a library folder");
        match initial_directory {
            Some(directory) => dialog.set_directory(directory).blocking_pick_folder(),
            None => dialog.blocking_pick_folder(),
        }
    })
    .await
    .map_err(|error| error.to_string())?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    let path = selected.into_path().map_err(|error| error.to_string())?;
    let approved = state
        .scanner
        .approve_root(&path)
        .map_err(|error| error.to_string())?;
    Ok(Some(SelectedScanRoot {
        access_handle: approved.access_handle,
        display_path: approved.display_path,
    }))
}

#[cfg(desktop)]
#[tauri::command]
pub async fn select_preferred_scan_root(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Option<ScanRootPreference>, String> {
    use tauri_plugin_dialog::DialogExt;

    let handle = app.clone();
    let initial_directory = state
        .scanner
        .read_root_preference()
        .ok()
        .filter(|preference| preference.available)
        .and_then(|preference| preference.display_path);
    let selected = tauri::async_runtime::spawn_blocking(move || {
        let dialog = handle
            .dialog()
            .file()
            .set_title("Choose the default library folder");
        match initial_directory {
            Some(directory) => dialog.set_directory(directory).blocking_pick_folder(),
            None => dialog.blocking_pick_folder(),
        }
    })
    .await
    .map_err(|error| error.to_string())?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    let path = selected.into_path().map_err(|error| error.to_string())?;
    state
        .scanner
        .configure_root(&path)
        .map(Some)
        .map_err(|error| error.to_string())
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
pub fn reset_scan_root_preference(
    state: tauri::State<'_, AppState>,
) -> Result<ScanRootPreference, String> {
    state
        .scanner
        .reset_root_preference()
        .map_err(|error| error.to_string())
}

#[cfg(desktop)]
#[tauri::command]
pub fn prepare_preferred_scan_root(
    state: tauri::State<'_, AppState>,
) -> Result<SelectedScanRoot, String> {
    let approved = state
        .scanner
        .approve_preferred_root()
        .map_err(|error| error.to_string())?;
    Ok(SelectedScanRoot {
        access_handle: approved.access_handle,
        display_path: approved.display_path,
    })
}

#[cfg(mobile)]
#[tauri::command]
pub fn prepare_preferred_scan_root(
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
    tauri::async_runtime::spawn_blocking(move || scanner.start(access_handle))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn cancel_library_scan(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<bool, String> {
    state
        .scanner
        .cancel(&ScanSessionId(session_id))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn read_latest_library_scan(
    state: tauri::State<'_, AppState>,
) -> Result<Option<ScanSessionView>, String> {
    let scanner = Arc::clone(&state.scanner);
    tauri::async_runtime::spawn_blocking(move || scanner.read_latest())
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn browse_library_scan_results(
    state: tauri::State<'_, AppState>,
    request: ScanResultRequest,
) -> Result<ScanResultPage, String> {
    let scanner = Arc::clone(&state.scanner);
    tauri::async_runtime::spawn_blocking(move || scanner.browse_results(request))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn browse_library_scan_issues(
    state: tauri::State<'_, AppState>,
    request: ScanIssueRequest,
) -> Result<ScanIssuePage, String> {
    let scanner = Arc::clone(&state.scanner);
    tauri::async_runtime::spawn_blocking(move || scanner.browse_issues(request))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn create_installation_from_scan(
    state: tauri::State<'_, AppState>,
    session_id: String,
    selected_result_id: String,
) -> Result<Installation, String> {
    let service = Arc::clone(&state.installation_from_scan);
    tauri::async_runtime::spawn_blocking(move || {
        service.create_or_refresh(CreateInstallationFromScanRequest {
            session_id: ScanSessionId(session_id),
            selected_result_id: ScanResultId(selected_result_id),
        })
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn list_library_installations(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Installation>, String> {
    let service = Arc::clone(&state.installation_review);
    tauri::async_runtime::spawn_blocking(move || service.list())
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn read_library_shelves(
    state: tauri::State<'_, AppState>,
) -> Result<LibraryShelves, String> {
    let service = Arc::clone(&state.library_shelves);
    tauri::async_runtime::spawn_blocking(move || service.read())
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn read_work_preference(
    state: tauri::State<'_, AppState>,
    work_code: String,
) -> Result<Option<WorkPreference>, String> {
    let service = Arc::clone(&state.work_preferences);
    tauri::async_runtime::spawn_blocking(move || service.read(&work_code))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn list_work_preferences(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<WorkPreference>, String> {
    let service = Arc::clone(&state.work_preferences);
    tauri::async_runtime::spawn_blocking(move || service.list())
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn replace_work_preference(
    state: tauri::State<'_, AppState>,
    work_code: String,
    preference: Option<WorkPreferenceKind>,
) -> Result<Option<WorkPreference>, String> {
    let service = Arc::clone(&state.work_preferences);
    tauri::async_runtime::spawn_blocking(move || {
        service.replace(&work_code, preference, &dla_sqlite::current_timestamp())
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn read_local_personalization(
    state: tauri::State<'_, AppState>,
) -> Result<LocalPersonalization, String> {
    let service = Arc::clone(&state.local_personalization);
    tauri::async_runtime::spawn_blocking(move || service.read())
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn list_library_installations_for_work(
    state: tauri::State<'_, AppState>,
    work_code: String,
) -> Result<Vec<Installation>, String> {
    let service = Arc::clone(&state.installation_review);
    tauri::async_runtime::spawn_blocking(move || service.list_for_work(&work_code))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn read_library_installation(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<Installation, String> {
    let service = Arc::clone(&state.installation_review);
    tauri::async_runtime::spawn_blocking(move || service.read(&InstallationId(installation_id)))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn save_library_installation_review(
    state: tauri::State<'_, AppState>,
    request: InstallationReviewRequest,
) -> Result<Installation, String> {
    let service = Arc::clone(&state.installation_review);
    tauri::async_runtime::spawn_blocking(move || {
        service.save(request, dla_sqlite::current_timestamp())
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
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
pub fn read_system_report() -> crate::system_report::SystemReport {
    crate::system_report::read_system_report()
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
