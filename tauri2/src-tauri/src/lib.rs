mod catalog_import;
mod commands;
mod cover_protocol;
mod media_protocol;
mod native_video;
mod package_preparation;
mod read_only_navigation;
mod scanner;
mod search;
mod support;
mod system_report;

use std::sync::Arc;

use dla_application::{
    android_app::{AndroidAppAssociationStore, AndroidAppService},
    android_package::AndroidPackageService,
    catalog::{CatalogReader, CatalogService},
    catalog_artwork::CatalogArtworkCache,
    catalog_import::{
        CatalogGenerationKind, CatalogGenerationState, CatalogGenerationSummary,
        CatalogImportService, CatalogPackageProfile,
    },
    identity::CatalogIdentityReader,
    installation::InstallationStore,
    installation_from_scan::{InstallationFromScanService, InstallationScanSource},
    installation_review::InstallationReviewService,
    launch::{LaunchActivityStore, LaunchClock, LaunchExecutor, LaunchService},
    library_shelves::{LibraryActivityReader, LibraryShelvesService},
    maintenance::{
        LibraryMaintenanceFilesystem, LibraryMaintenanceService, LibraryMaintenanceStore,
    },
    media::{
        AudioTrackStore, AudioWaveformReader, MediaInventoryReader, MediaService,
        MediaSessionStore, PersonalizedVoiceQueueService,
    },
    package_inspection::PackageManifestReader,
    package_preparation::{PackageInstaller, PackagePreparationService, PackagePreparationStore},
    personalization::{
        ContextualRecommendationProvider, LocalPersonalizationService, WorkPreferenceService,
        WorkPreferenceStore,
    },
    recommendation::{CatalogRecommendationReader, CatalogRecommendationService},
    scan_execution::ScanExecutionService,
    scanner::{
        ArchiveHasher, FilesystemScanner, ScanClock, ScanIdentifiers, ScanRepository,
        ScanRootLocationProvider, ScanRootPreferenceRepository, ScanRootPreferenceService,
    },
    search::{CatalogIndexSource, CatalogSearchIndex, CatalogSearchReader, CatalogSearchService},
};
use dla_archive::{
    DesktopLibraryMaintenance, DesktopPackageInstaller, DesktopPackageManifestReader,
};
use dla_catalog_import::{
    CatalogImportAdapter, CatalogPackageAccessRegistry, all_fields, resolve_catalog_path,
};
use dla_cover_cache::DesktopCatalogArtworkCache;
use dla_launch::{DesktopLaunchExecutor, SystemLaunchClock};
use dla_media::{DesktopAudioWaveformReader, DesktopMediaInventory};
use dla_scanner::{
    DesktopFilesystem, DesktopScanRootLocations, ScanAccessRegistry, SystemScanClock,
    SystemScanIdentifiers,
};
use dla_search_tantivy::TantivyCatalogSearch;
use dla_sqlite::{
    ReloadableCatalogStore, SqliteCatalogStore, SqliteLibraryStore, StoredCatalogGeneration,
    current_timestamp, database_size,
};
use tauri::Manager;

use catalog_import::CatalogImportController;
use commands::AppState;
use package_preparation::PackagePreparationController;
use scanner::{ScanController, TauriScanProgressSink};
use search::SearchIndexController;
use support::SupportRuntime;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let arguments = std::env::args_os().collect::<Vec<_>>();
    let diagnostic_mode = arguments
        .iter()
        .any(|argument| argument == "--diagnostic-report");
    let builder = tauri::Builder::default()
        .plugin(dla_android_package_tauri::init())
        .manage(read_only_navigation::ReadOnlyNavigationState::from_cli_arguments(&arguments));
    #[cfg(desktop)]
    let builder = if diagnostic_mode {
        builder
    } else {
        builder.plugin(tauri_plugin_single_instance::init(|app, arguments, _| {
            read_only_navigation::deliver_cli_arguments(app, &arguments);
            reveal_main_window(app);
        }))
    };
    let builder = builder
        .plugin(tauri_plugin_deep_link::init())
        .register_asynchronous_uri_scheme_protocol("dla-media", |context, request, responder| {
            let app = context.app_handle().clone();
            std::thread::spawn(move || {
                responder.respond(media_protocol::respond(&app, request));
            });
        })
        .register_asynchronous_uri_scheme_protocol("dla-video", |context, request, responder| {
            let app = context.app_handle().clone();
            std::thread::spawn(move || {
                responder.respond(media_protocol::respond_video_document(&app, request));
            });
        })
        .register_asynchronous_uri_scheme_protocol("dla-subtitle", |context, request, responder| {
            let app = context.app_handle().clone();
            std::thread::spawn(move || {
                responder.respond(media_protocol::respond_subtitle(&app, request));
            });
        })
        .register_asynchronous_uri_scheme_protocol("dla-cover", |context, request, responder| {
            let app = context.app_handle().clone();
            std::thread::spawn(move || {
                responder.respond(cover_protocol::respond(&app, request));
            });
        })
        .plugin(tauri_plugin_opener::init());
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_dialog::init());
    let app = builder
        .setup(move |app| {
            let support = SupportRuntime::initialize(app, !diagnostic_mode)
                .map_err(std::io::Error::other)?;
            if let Err(error) = support::install_logging(app, &support) {
                eprintln!("DLA Launcher could not initialize file logging: {error}");
            }
            support::install_panic_capture(support.clone());
            app.manage(support.clone());
            #[cfg(target_os = "linux")]
            install_linux_termination_handlers(app.handle());
            log::info!(
                target: "dla::lifecycle",
                "event=startup_begin run_id={} diagnostic_mode={diagnostic_mode}",
                support.run_id()
            );

            if diagnostic_mode {
                #[cfg(desktop)]
                {
                    if let Some(window) = app.get_webview_window("main") {
                        window.hide()?;
                    }
                    let handle = app.handle().clone();
                    tauri::async_runtime::spawn(async move {
                        let exit_code = match support::save_with_dialog(handle.clone(), support).await {
                            Ok(_) => 0,
                            Err(error) => {
                                eprintln!("DLA Launcher could not save the diagnostic report: {error}");
                                1
                            }
                        };
                        handle.exit(exit_code);
                    });
                    return Ok(());
                }
                #[cfg(mobile)]
                return Err(std::io::Error::other(
                    "diagnostic report mode is available only on desktop",
                )
                .into());
            }

            #[cfg(target_os = "linux")]
            {
                use tauri_plugin_deep_link::DeepLinkExt;

                if let Err(error) = app.deep_link().register_all() {
                    log::warn!(target: "dla::navigation", "event=deep_link_registration_failed error={error}");
                }
            }

            #[cfg(any(target_os = "android", target_os = "macos"))]
            read_only_navigation::install_open_url_listener(app);

            let product_result = (|| -> Result<(), Box<dyn std::error::Error>> {
            #[cfg(target_os = "linux")]
            enable_media_protocol_cors(app)?;

            let data_directory = app.path().app_data_dir()?;
            let cache_directory = app.path().app_cache_dir()?;
            let cover_cache: Arc<dyn CatalogArtworkCache> = Arc::new(
                DesktopCatalogArtworkCache::open(
                    cache_directory.join("covers"),
                    data_directory.join("preferences/cover-cache.json"),
                )?,
            );
            let library = Arc::new(SqliteLibraryStore::open(
                &data_directory.join("library.sqlite"),
            )?);
            library.health_check()?;
            let fixture = dla_catalog::empty();
            let embedded_path = data_directory.join("catalog.sqlite");
            let embedded_store = Arc::new(SqliteCatalogStore::open(&embedded_path, &fixture)?);
            library.initialize_embedded_catalog(&StoredCatalogGeneration {
                summary: CatalogGenerationSummary {
                    id: "embedded".to_owned(),
                    snapshot_id: fixture.snapshot_id.clone(),
                    kind: CatalogGenerationKind::Embedded,
                    state: CatalogGenerationState::Available,
                    profile: CatalogPackageProfile::Custom,
                    source_name: "Empty catalog baseline".to_owned(),
                    package_name: String::new(),
                    imported_at: current_timestamp(),
                    work_count: fixture.works.len() as u64,
                    rom_count: fixture
                        .works
                        .iter()
                        .map(|work| work.roms.len() as u64)
                        .sum(),
                    database_bytes: database_size(&embedded_path),
                    fields: all_fields().into_iter().map(str::to_owned).collect(),
                    failure_detail: String::new(),
                },
                catalog_path: "catalog.sqlite".to_owned(),
            })?;
            let active_generation = library.read_active_catalog_generation()?;
            let active_store = if active_generation.summary.id == "embedded" {
                embedded_store
            } else {
                let active_path =
                    resolve_catalog_path(&data_directory, &active_generation.catalog_path)?;
                match SqliteCatalogStore::open_existing(&active_path) {
                    Ok(store) => Arc::new(store),
                    Err(error) => {
                        library.mark_catalog_generation_failed(
                            &active_generation.summary.id,
                            &error.to_string(),
                        )?;
                        library.activate_catalog_generation("embedded")?;
                        Arc::new(SqliteCatalogStore::open(&embedded_path, &fixture)?)
                    }
                }
            };
            let catalog_store = Arc::new(ReloadableCatalogStore::new(active_store));
            let catalog_reader: Arc<dyn CatalogReader> = catalog_store.clone();
            let recommendation_reader: Arc<dyn CatalogRecommendationReader> = catalog_store.clone();
            let index_source: Arc<dyn CatalogIndexSource> = catalog_store.clone();
            let catalog_identity: Arc<dyn CatalogIdentityReader> = catalog_store.clone();
            let scanner_catalog_identity = Arc::clone(&catalog_identity);
            let search_reader: Arc<dyn CatalogSearchReader> = catalog_store.clone();
            let search_index: Arc<dyn CatalogSearchIndex> = Arc::new(TantivyCatalogSearch::open(
                cache_directory.join("search/catalog"),
            )?);
            let search = Arc::new(CatalogSearchService::new(
                index_source,
                Arc::clone(&catalog_identity),
                search_reader,
                search_index,
            ));
            let search_index_controller = Arc::new(SearchIndexController::new(
                Arc::clone(&search),
                app.handle().clone(),
            ));
            let catalog_package_access = Arc::new(CatalogPackageAccessRegistry::new());
            let catalog_import_adapter = Arc::new(CatalogImportAdapter::new(
                data_directory.clone(),
                Arc::clone(&catalog_package_access),
                Arc::clone(&catalog_store),
                Arc::clone(&library),
                Arc::clone(&search),
            )?);
            let catalog_import = Arc::new(CatalogImportController::new(
                Arc::new(CatalogImportService::new(catalog_import_adapter)),
                catalog_package_access,
                app.handle().clone(),
            ));
            let scan_repository: Arc<dyn ScanRepository> = library.clone();
            let scan_preference_repository: Arc<dyn ScanRootPreferenceRepository> = library.clone();
            let scan_root_locations: Arc<dyn ScanRootLocationProvider> =
                Arc::new(DesktopScanRootLocations::new());
            let scan_root_preferences = Arc::new(ScanRootPreferenceService::new(
                scan_preference_repository,
                scan_root_locations,
            ));
            let scan_access = Arc::new(ScanAccessRegistry::new());
            let desktop_filesystem = Arc::new(DesktopFilesystem::new(Arc::clone(&scan_access)));
            let filesystem_scanner: Arc<dyn FilesystemScanner> = desktop_filesystem.clone();
            let archive_hasher: Arc<dyn ArchiveHasher> = desktop_filesystem;
            let scan_clock: Arc<dyn ScanClock> = Arc::new(SystemScanClock);
            let scan_identifiers: Arc<dyn ScanIdentifiers> = Arc::new(SystemScanIdentifiers);
            let scan_progress = Arc::new(TauriScanProgressSink::new(app.handle().clone()));
            let scanner_service = Arc::new(ScanExecutionService::new(
                filesystem_scanner,
                archive_hasher,
                scanner_catalog_identity,
                scan_repository,
                scan_progress,
                scan_clock,
                scan_identifiers,
            ));
            scanner_service.interrupt_active_sessions()?;
            let scanner = Arc::new(ScanController::new(
                scanner_service,
                scan_access,
                scan_root_preferences,
            ));
            let installation_scan_source: Arc<dyn InstallationScanSource> = library.clone();
            let installation_store: Arc<dyn InstallationStore> = library.clone();
            let package_manifest_reader: Arc<dyn PackageManifestReader> =
                Arc::new(DesktopPackageManifestReader::new());
            let library_activity: Arc<dyn LibraryActivityReader> = library.clone();
            let library_shelves = Arc::new(LibraryShelvesService::new(
                Arc::clone(&installation_store),
                Arc::clone(&library_activity),
            ));
            let preference_store: Arc<dyn WorkPreferenceStore> = library.clone();
            let work_preferences =
                Arc::new(WorkPreferenceService::new(Arc::clone(&preference_store)));
            let recommendations =
                Arc::new(CatalogRecommendationService::new(recommendation_reader));
            let recommendation_provider: Arc<dyn ContextualRecommendationProvider> =
                recommendations.clone();
            let local_personalization = Arc::new(LocalPersonalizationService::new(
                Arc::clone(&preference_store),
                Arc::clone(&installation_store),
                Arc::clone(&library_activity),
                Arc::clone(&catalog_reader),
                recommendation_provider,
            ));
            let installation_from_scan =
                Arc::new(InstallationFromScanService::with_package_inspection(
                    installation_scan_source,
                    Arc::clone(&installation_store),
                    Arc::clone(&package_manifest_reader),
                    Arc::clone(&catalog_reader),
                ));
            let installation_review = Arc::new(InstallationReviewService::new(
                Arc::clone(&catalog_identity),
                Arc::clone(&installation_store),
            ));
            let package_preparation_store: Arc<dyn PackagePreparationStore> = library.clone();
            let package_installer: Arc<dyn PackageInstaller> =
                Arc::new(DesktopPackageInstaller::new());
            let maintenance_store: Arc<dyn LibraryMaintenanceStore> = library.clone();
            let maintenance_filesystem: Arc<dyn LibraryMaintenanceFilesystem> =
                Arc::new(DesktopLibraryMaintenance::new());
            let maintenance = Arc::new(LibraryMaintenanceService::new(
                Arc::clone(&installation_store),
                Arc::clone(&package_preparation_store),
                maintenance_store,
                maintenance_filesystem,
                package_manifest_reader,
                Arc::clone(&package_installer),
            ));
            let launch_executor: Arc<dyn LaunchExecutor> = Arc::new(DesktopLaunchExecutor::new());
            let launch_activity_store: Arc<dyn LaunchActivityStore> = library.clone();
            let launch_clock: Arc<dyn LaunchClock> = Arc::new(SystemLaunchClock);
            let launch = Arc::new(LaunchService::new(
                Arc::clone(&installation_store),
                Arc::clone(&package_preparation_store),
                launch_executor,
                launch_activity_store,
                launch_clock,
            ));
            launch.reconcile_after_restart()?;
            let media_sessions: Arc<dyn MediaSessionStore> = library.clone();
            let audio_tracks: Arc<dyn AudioTrackStore> = library.clone();
            let media_inventory: Arc<dyn MediaInventoryReader> =
                Arc::new(DesktopMediaInventory::new());
            let audio_waveforms: Arc<dyn AudioWaveformReader> = Arc::new(
                DesktopAudioWaveformReader::new(cache_directory.join("media/waveforms")),
            );
            let media = Arc::new(MediaService::new(
                Arc::clone(&installation_store),
                Arc::clone(&package_preparation_store),
                Arc::clone(&media_sessions),
                audio_tracks,
                media_inventory,
                audio_waveforms,
            ));
            media.reconcile_after_restart(&current_timestamp())?;
            let personalized_voice = Arc::new(PersonalizedVoiceQueueService::new(
                Arc::clone(&media),
                Arc::clone(&installation_store),
                media_sessions,
                preference_store,
                library_activity,
            ));
            let package_preparation = Arc::new(PackagePreparationController::new(
                Arc::new(PackagePreparationService::new(
                    Arc::clone(&installation_store),
                    Arc::clone(&package_preparation_store),
                    package_installer,
                )),
                app.handle().clone(),
            ));
            let android_package = Arc::new(AndroidPackageService::new(Arc::clone(
                &app.state::<dla_android_package_tauri::AndroidPackagePlatformState>().0,
            )));
            let android_app_store: Arc<dyn AndroidAppAssociationStore> = library.clone();
            let android_app = Arc::new(AndroidAppService::new(
                android_app_store,
                catalog_identity,
                Arc::clone(
                    &app.state::<dla_android_package_tauri::AndroidAppPlatformState>().0,
                ),
            ));

            app.manage(AppState {
                android_app,
                android_package,
                catalog: Arc::new(CatalogService::new(catalog_reader)),
                cover_cache,
                recommendations,
                search,
                search_index_controller,
                scanner,
                catalog_import,
                installation_from_scan,
                installation_review,
                library_shelves,
                work_preferences,
                local_personalization,
                package_preparation,
                maintenance,
                launch,
                media,
                personalized_voice,
                native_video: Arc::new(native_video::NativeVideoController::default()),
            });
            Ok(())
            })();
            match product_result {
                Ok(()) => {
                    if let Err(error) = support.mark_startup_ready() {
                        log::warn!(target: "dla::support", "event=run_marker_update_failed error={error}");
                    }
                    Ok(())
                }
                Err(error) => {
                    support.record_startup_failure(&error.to_string());
                    Err(error)
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::browse_catalog,
            commands::read_catalog_context,
            commands::read_catalog_work,
            commands::read_catalog_works,
            commands::read_catalog_recommendations,
            commands::read_catalog_rom_contents,
            commands::read_android_package_state,
            commands::list_android_apps,
            commands::associate_installed_android_app,
            commands::launch_android_app,
            commands::remove_android_app_association,
            commands::select_and_inspect_android_package,
            commands::clear_android_package_selection,
            commands::open_android_package_source_approval,
            commands::request_android_package_install,
            commands::read_cover_cache_summary,
            commands::configure_cover_cache,
            commands::read_system_report,
            commands::read_search_index_status,
            commands::rebuild_search_index,
            commands::cancel_search_index_rebuild,
            commands::read_search_index_rebuild_progress,
            commands::cleanup_search_index_cache,
            commands::search_catalog,
            commands::search_catalog_shortcuts,
            commands::read_scan_root_preference,
            commands::select_scan_root,
            commands::select_preferred_scan_root,
            commands::reset_scan_root_preference,
            commands::prepare_preferred_scan_root,
            commands::start_library_scan,
            commands::cancel_library_scan,
            commands::read_latest_library_scan,
            commands::browse_library_scan_results,
            commands::browse_library_scan_issues,
            commands::create_installation_from_scan,
            commands::list_library_installations,
            commands::read_library_shelves,
            commands::read_work_preference,
            commands::list_work_preferences,
            commands::replace_work_preference,
            commands::read_local_personalization,
            commands::list_library_installations_for_work,
            commands::read_library_installation,
            commands::save_library_installation_review,
            commands::select_installation_destination,
            commands::select_installation_location,
            commands::read_prepared_package,
            commands::read_prepared_packages,
            commands::read_installation_health,
            commands::read_installation_healths,
            commands::verify_library_installation,
            commands::locate_library_installation,
            commands::rescan_library_installation,
            commands::repair_library_installation,
            commands::remove_library_installation,
            commands::uninstall_library_installation,
            commands::cleanup_library_maintenance,
            commands::inspect_package_destination,
            commands::start_package_preparation,
            commands::cancel_package_preparation,
            commands::read_package_preparation_progress,
            commands::launch_library_installation,
            commands::stop_library_launch,
            commands::list_recent_launches,
            commands::list_installation_launch_history,
            commands::open_library_media_session,
            commands::list_library_media_items,
            commands::read_library_audio_waveform,
            commands::open_personalized_voice_queue,
            commands::read_library_media_session,
            commands::update_library_media_progress,
            commands::update_library_media_queue_settings,
            commands::close_library_media_session,
            commands::open_native_video,
            commands::update_native_video_viewport,
            commands::control_native_video,
            commands::close_native_video,
            commands::select_catalog_package,
            commands::inspect_catalog_package,
            commands::start_catalog_import,
            commands::cancel_catalog_import,
            commands::read_catalog_import_progress,
            commands::list_catalog_generations,
            commands::activate_catalog_generation,
            commands::remove_catalog_generation,
            commands::read_window_metrics,
            commands::resize_window,
            commands::maximize_window,
            support::read_support_status,
            support::acknowledge_unclean_shutdown,
            support::record_frontend_fault,
            support::save_support_bundle,
            support::open_support_issue,
            support::open_support_project,
            read_only_navigation::read_current_read_only_deep_links,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Tauri application");
    app.run(|app, event| {
        if matches!(event, tauri::RunEvent::Ready) {
            read_only_navigation::signal_runtime_ready(app);
        }
        if matches!(event, tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. })
            && let Some(support) = app.try_state::<SupportRuntime>()
            && support.mark_clean_shutdown()
        {
            log::info!(target: "dla::lifecycle", "event=clean_shutdown run_id={}", support.run_id());
        }
    });
}

#[cfg(target_os = "linux")]
fn install_linux_termination_handlers(app: &tauri::AppHandle) {
    for signal in [libc::SIGINT, libc::SIGTERM] {
        let app = app.clone();
        gtk::glib::source::unix_signal_add_once(signal, move || {
            if let Some(support) = app.try_state::<SupportRuntime>() {
                log::info!(
                    target: "dla::lifecycle",
                    "event=termination_requested signal={signal} run_id={}",
                    support.run_id()
                );
                let _ = support.mark_clean_shutdown();
            }
            app.exit(0);
        });
    }
}

#[cfg(target_os = "linux")]
fn enable_media_protocol_cors(app: &tauri::App) -> tauri::Result<()> {
    use webkit2gtk::{SecurityManagerExt, WebContextExt, WebViewExt};

    if let Some(main_webview) = app.get_webview_window("main") {
        main_webview.with_webview(|webview| {
            if let Some(context) = webview.inner().context()
                && let Some(security_manager) = context.security_manager()
            {
                security_manager.register_uri_scheme_as_cors_enabled("dla-media");
                security_manager.register_uri_scheme_as_local("dla-video");
                security_manager.register_uri_scheme_as_secure("dla-video");
                security_manager.register_uri_scheme_as_cors_enabled("dla-video");
                security_manager.register_uri_scheme_as_local("dla-subtitle");
                security_manager.register_uri_scheme_as_secure("dla-subtitle");
                security_manager.register_uri_scheme_as_cors_enabled("dla-subtitle");
                security_manager.register_uri_scheme_as_cors_enabled("dla-cover");
            }
        })?;
    }
    Ok(())
}

#[cfg(desktop)]
fn reveal_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let main_window = window.clone();
        let _ = window.run_on_main_thread(move || {
            #[cfg(target_os = "linux")]
            {
                use gtk::prelude::{GtkWindowExt, WidgetExt};

                if let Ok(native_window) = main_window.gtk_window() {
                    native_window.show_all();
                    native_window.present();
                }
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = main_window.show();
                let _ = main_window.unminimize();
                let _ = main_window.set_focus();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn content_security_policy_allows_private_audio_materialization() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("Tauri config");
        let security = &config["app"]["security"];
        for field in ["csp", "devCsp"] {
            let policy = security[field].as_str().expect("content security policy");
            let connect = policy
                .split(';')
                .find(|directive| directive.trim_start().starts_with("connect-src "))
                .expect("connect-src directive");
            assert!(connect.contains("dla-media:"));
            assert!(connect.contains("http://dla-media.localhost"));
            let media = policy
                .split(';')
                .find(|directive| directive.trim_start().starts_with("media-src "))
                .expect("media-src directive");
            assert!(media.contains("blob:"));
        }
    }

    #[test]
    fn linux_release_targets_exclude_the_unverified_appimage() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.linux.conf.json")).expect("Linux config");
        assert_eq!(
            config["bundle"]["targets"],
            serde_json::json!(["deb", "rpm"])
        );
    }
}
