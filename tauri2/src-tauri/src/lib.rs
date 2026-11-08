mod catalog_import;
#[cfg(feature = "chromium")]
mod chromium;
mod commands;
mod cover_protocol;
#[cfg(desktop)]
mod delivery;
#[cfg(desktop)]
mod delivery_access;
mod launch;
#[cfg(desktop)]
mod library_add;
mod media_protocol;
mod native_video;
mod operation;
mod package_preparation;
mod protocol_workers;
mod read_only_navigation;
mod render_recovery;
mod scan_installs;
mod scanner;
mod search;
mod support;
mod system_report;
mod translation;

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
    delivery::InstallFolderStore,
    game_translation::{GameFolder, GameTranslationService, GameTranslationStore},
    identity::CatalogIdentityReader,
    installation::InstallationStore,
    installation_from_scan::{InstallationFromScanService, InstallationScanSource},
    installation_review::InstallationReviewService,
    launch::{
        LaunchActivityStore, LaunchClock, LaunchExecutor, LaunchOptionStore, LaunchService,
        ObservedLaunchActivityStore,
    },
    library_shelves::{LibraryActivityReader, LibraryShelvesService},
    maintenance::{
        InstalledFileNames, InstalledPathStore, LauncherFileChanges, LibraryMaintenanceFilesystem,
        LibraryMaintenanceService, LibraryMaintenanceStore,
    },
    media::{
        AudioTrackStore, AudioWaveformReader, MediaInventoryReader, MediaService,
        MediaSessionStore, PersonalizedVoiceQueueService,
    },
    own_work::{OwnWorkArtworkStore, OwnWorkDetailsStore, OwnWorkService},
    package_inspection::PackageManifestReader,
    package_preparation::{PackageInstaller, PackagePreparationService, PackagePreparationStore},
    personalization::{
        ContextualRecommendationProvider, LocalPersonalizationService, WorkPreferenceService,
        WorkPreferenceStore,
    },
    recommendation::{CatalogRecommendationReader, CatalogRecommendationService},
    scan_execution::ScanExecutionService,
    scan_findings::{IgnoredScanFindingStore, ScanFindingService},
    scanner::{
        ArchiveHasher, FilesystemScanner, ScanClock, ScanIdentifiers, ScanRepository,
        ScanRootLocationProvider, ScanRootPreferenceRepository, ScanRootPreferenceService,
    },
    search::{CatalogIndexSource, CatalogSearchIndex, CatalogSearchReader, CatalogSearchService},
    translation::{TranslationService, TranslationStore},
};
use dla_archive::{
    DesktopInstalledFileNames, DesktopLibraryMaintenance, DesktopPackageInstaller,
    DesktopPackageManifestReader,
};
use dla_catalog_import::{
    CatalogImportAdapter, CatalogPackageAccessRegistry, all_fields, resolve_catalog_path,
};
use dla_cover_cache::DesktopCatalogArtworkCache;
use dla_game_translation::DesktopGameFolder;
use dla_launch::{DesktopLaunchExecutor, LocaleRemulator, SystemLaunchClock};
use dla_media::{
    DesktopAudioWaveformReader, DesktopMediaInventory, OwnWorkArtworkFiles, THUMBNAIL_CACHE_LIMIT,
    ThumbnailCache,
};
use dla_scanner::{
    DesktopFilesystem, DesktopScanRootLocations, ScanAccessRegistry, SystemScanClock,
    SystemScanIdentifiers,
};
use dla_search_tantivy::TantivyCatalogSearch;
use dla_sqlite::{
    ReloadableCatalogStore, SqliteCatalogStore, SqliteLibraryStore, SqliteTranslationStore,
    StoredCatalogGeneration, current_timestamp, database_size,
};
use dla_translator::TranslatorSettingsFile;
use tauri::Manager;

use catalog_import::CatalogImportController;
use commands::AppState;
use package_preparation::PackagePreparationController;
use protocol_workers::ProtocolWorkers;
use scanner::{ScanController, TauriScanProgressSink};
use search::SearchIndexController;
use support::SupportRuntime;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
// Finished scan and media sessions older than this are history the launcher
// no longer shows; the prune waits until the first screens have loaded,
// since it holds the library lock while it runs.
const HISTORY_RETENTION: time::Duration = time::Duration::days(30);
const HISTORY_PRUNE_DELAY: std::time::Duration = std::time::Duration::from_secs(20);

pub fn run() {
    let arguments = std::env::args_os().collect::<Vec<_>>();
    let diagnostic_mode = arguments
        .iter()
        .any(|argument| argument == "--diagnostic-report");
    let context = tauri::generate_context!();
    let builder = tauri::Builder::default();
    #[cfg(feature = "chromium")]
    let builder = builder.runtime(chromium::runtime(&context.config().identifier));
    let builder = builder
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
    // Artwork and media get separate pools so a burst of cover downloads can
    // never delay the audio or video the user is already playing.
    let cover_workers =
        Arc::new(ProtocolWorkers::start("dla-cover", 8).expect("cover protocol worker threads"));
    let media_workers =
        Arc::new(ProtocolWorkers::start("dla-media", 4).expect("media protocol worker threads"));
    let media_protocol_workers = Arc::clone(&media_workers);
    let video_protocol_workers = Arc::clone(&media_workers);
    let builder = builder
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_deep_link::init())
        .register_asynchronous_uri_scheme_protocol(
            "dla-media",
            move |context, request, responder| {
                let app = context.app_handle().clone();
                media_protocol_workers.run(move || {
                    responder.respond(media_protocol::respond(&app, request));
                });
            },
        )
        .register_asynchronous_uri_scheme_protocol(
            "dla-video",
            move |context, request, responder| {
                let app = context.app_handle().clone();
                video_protocol_workers.run(move || {
                    responder.respond(media_protocol::respond_video_document(&app, request));
                });
            },
        )
        .register_asynchronous_uri_scheme_protocol(
            "dla-subtitle",
            move |context, request, responder| {
                let app = context.app_handle().clone();
                media_workers.run(move || {
                    responder.respond(media_protocol::respond_subtitle(&app, request));
                });
            },
        )
        .register_asynchronous_uri_scheme_protocol(
            "dla-cover",
            move |context, request, responder| {
                let app = context.app_handle().clone();
                cover_workers.run(move || {
                    responder.respond(cover_protocol::respond(&app, request));
                });
            },
        )
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
            #[cfg(all(feature = "chromium", target_os = "linux"))]
            chromium::remove_idle_glib_work_source();
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
            #[cfg(all(target_os = "linux", not(feature = "chromium")))]
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
            #[cfg(desktop)]
            let known_digests = Arc::new(delivery::KnownDigests::default());
            #[cfg(desktop)]
            let archive_hasher: Arc<dyn ArchiveHasher> = Arc::new(delivery::KnownDigestHasher::new(
                desktop_filesystem,
                Arc::clone(&scan_access),
                Arc::clone(&known_digests),
            ));
            #[cfg(mobile)]
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
                    Arc::clone(&installation_scan_source),
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
            let launcher_file_changes: Arc<dyn LauncherFileChanges> = library.clone();
            let installed_file_names: Arc<dyn InstalledFileNames> =
                Arc::new(DesktopInstalledFileNames::new());
            let installed_paths: Arc<dyn InstalledPathStore> = library.clone();
            let maintenance = Arc::new(
                LibraryMaintenanceService::new(
                    Arc::clone(&installation_store),
                    Arc::clone(&package_preparation_store),
                    maintenance_store,
                    maintenance_filesystem,
                    package_manifest_reader,
                    Arc::clone(&package_installer),
                )
                .with_launcher_changes(launcher_file_changes)
                .with_file_names(installed_file_names, installed_paths),
            );
            let locale_remulator =
                LocaleRemulator::new(data_directory.join("tools").join("locale-remulator"));
            let launch_executor: Arc<dyn LaunchExecutor> = Arc::new(
                DesktopLaunchExecutor::new().with_locale_remulator(locale_remulator.clone()),
            );
            let launch_activity_store: Arc<dyn LaunchActivityStore> =
                Arc::new(ObservedLaunchActivityStore::new(
                    library.clone(),
                    Arc::new(launch::TauriLaunchActivityEvents::new(app.handle().clone())),
                ));
            let launch_clock: Arc<dyn LaunchClock> = Arc::new(SystemLaunchClock);
            let launch_options: Arc<dyn LaunchOptionStore> = library.clone();
            let launch = Arc::new(
                LaunchService::new(
                    Arc::clone(&installation_store),
                    Arc::clone(&package_preparation_store),
                    launch_executor,
                    launch_activity_store,
                    launch_clock,
                )
                .with_launch_options(Arc::clone(&launch_options)),
            );
            launch.reconcile_after_restart()?;
            #[cfg(desktop)]
            let (delivery_client, delivery_forward) = {
                use dla_delivery::{DeliveryClient, Endpoint};

                // A plain-HTTP loopback stand-in for the self-test, else the
                // private route when this build carries the launcher token.
                let self_test = std::env::var("DLA_LAUNCHER_DOWNLOADS")
                    .ok()
                    .filter(|value| !value.trim().is_empty());
                let (endpoint, forward) = match (self_test, delivery_access::embedded()) {
                    (Some(base), _) => (Some(Endpoint::self_test(&base)?), None),
                    (None, Some(access)) => {
                        let forward = Arc::new(delivery::Forward::new(
                            access,
                            delivery::cloudflared_sidecar(),
                        ));
                        let ca: Arc<[u8]> =
                            Arc::from(&include_bytes!("../../certs/dla-upload-ca.crt")[..]);
                        (
                            Some(Endpoint::private(ca).through(forward.dial())),
                            Some(forward),
                        )
                    }
                    (None, None) => (None, None),
                };
                let client = endpoint.map(DeliveryClient::new).transpose()?.map(Arc::new);
                (client, forward)
            };
            #[cfg(desktop)]
            let overlay_source = {
                use dla_delivery::{Endpoint, OverlayClient};
                let ca = &include_bytes!(concat!(env!("OUT_DIR"), "/overlays-ca.crt"))[..];
                let self_test = std::env::var("DLA_LAUNCHER_OVERLAYS").ok()
                    .filter(|value| !value.trim().is_empty());
                let (endpoint, forward) = match (self_test, delivery_access::overlays()) {
                    (Some(base), _) => (Some(Endpoint::self_test(&base)?), None),
                    (None, Some(access)) if !ca.is_empty() => {
                        let forward = Arc::new(delivery::Forward::new(access, delivery::cloudflared_sidecar()));
                        (Some(Endpoint::overlays(Arc::from(ca)).through(forward.dial())), Some(forward))
                    }
                    _ => (None, None),
                };
                match (endpoint, delivery_client.as_ref()) {
                    (Some(endpoint), Some(session)) => Some((Arc::new(OverlayClient::new(endpoint, Arc::clone(session))?), forward)),
                    _ => None,
                }
            };
            let translation_store: Arc<dyn TranslationStore> = Arc::new(
                SqliteTranslationStore::open(&data_directory.join("translations.sqlite"))?,
            );
            let translation_service = Arc::new(
                TranslationService::new(translation_store)
                    .with_name_readings(Arc::new(dla_translator::NameList::new())),
            );
            let game_translation_store: Arc<dyn GameTranslationStore> = library.clone();
            let game_folder: Arc<dyn GameFolder> = Arc::new(DesktopGameFolder::new());
            let mut game_translations = GameTranslationService::new(
                Arc::clone(&installation_store),
                Arc::clone(&package_preparation_store),
                game_translation_store,
                game_folder,
                Arc::clone(&translation_service),
            );
            if let Some(packs) = translation::pack_source(
                #[cfg(desktop)]
                overlay_source,
            ) {
                game_translations = game_translations.with_packs(packs);
            }
            let translation = Arc::new(translation::TranslationController::new(
                translation_service,
                Arc::new(game_translations),
                TranslatorSettingsFile::new(data_directory.join("preferences/translation.json")),
                Arc::clone(&launch),
                app.handle().clone(),
            ));
            let media_sessions: Arc<dyn MediaSessionStore> = library.clone();
            let audio_tracks: Arc<dyn AudioTrackStore> = library.clone();
            let media_inventory: Arc<dyn MediaInventoryReader> =
                Arc::new(DesktopMediaInventory::new());
            let own_work_details: Arc<dyn OwnWorkDetailsStore> = library.clone();
            let own_work_artwork: Arc<dyn OwnWorkArtworkStore> = Arc::new(
                OwnWorkArtworkFiles::new(data_directory.join("library/artwork")),
            );
            let own_works = Arc::new(OwnWorkService::new(
                Arc::clone(&installation_store),
                Arc::clone(&package_preparation_store),
                Arc::clone(&media_inventory),
                own_work_details,
                own_work_artwork,
            ));
            let orphans = Arc::clone(&own_works);
            if let Err(error) = std::thread::Builder::new()
                .name("dla-own-work-artwork-prune".to_owned())
                .spawn(move || {
                    if let Err(error) = orphans.remove_orphaned_artwork() {
                        log::warn!("own work artwork cleanup failed: {error}");
                    }
                })
            {
                log::warn!("own work artwork cleanup could not start: {error}");
            }
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
            #[cfg(desktop)]
            let delivery = {
                use dla_application::delivery::{DeliveryService, DeliveryStore};
                use dla_delivery::{DeliveryClient, Endpoint, FileTransfer, ManifestLocator};

                let (client, forward) = (delivery_client, delivery_forward);
                // Without a route the service still lists and removes jobs;
                // nothing can sign in, so nothing is downloaded.
                let remote = match &client {
                    Some(client) => Arc::clone(client),
                    None => Arc::new(DeliveryClient::new(Endpoint::self_test(
                        "http://127.0.0.1:9/v1",
                    )?)?),
                };
                let delivery_store: Arc<dyn DeliveryStore> = library.clone();
                let install_folders: Arc<dyn InstallFolderStore> = library.clone();
                let service = Arc::new(DeliveryService::new(
                    Arc::clone(&catalog_reader),
                    delivery_store,
                    install_folders,
                    Arc::new(ManifestLocator::new(
                        Arc::clone(&remote),
                        Some(data_directory.join("delivery/manifests.json")),
                    )),
                    Arc::new(FileTransfer::new(remote)),
                    Arc::new(delivery::ShellInstaller::new(
                        Arc::clone(&scanner),
                        Arc::clone(&installation_from_scan),
                        Arc::clone(&package_preparation),
                        Arc::clone(&installation_review),
                        known_digests,
                    )),
                ));
                service.reconcile_after_restart(&current_timestamp())?;
                Arc::new(delivery::DeliveryController::new(
                    service,
                    client,
                    forward,
                    app.handle().clone(),
                ))
            };
            let ignored_findings: Arc<dyn IgnoredScanFindingStore> = library.clone();
            let scan_findings = Arc::new(ScanFindingService::new(
                installation_scan_source,
                ignored_findings,
                Arc::clone(&installation_store),
                installation_from_scan,
                Arc::clone(&installation_review),
            ));
            let install_folders: Arc<dyn InstallFolderStore> = library.clone();
            let scan_installs = Arc::new(scan_installs::ScanInstallController::new(
                Arc::clone(&scan_findings),
                Arc::clone(&package_preparation),
                Arc::clone(&installation_review),
                Arc::clone(&maintenance),
                Arc::clone(&install_folders),
                app.handle().clone(),
            ));
            #[cfg(desktop)]
            let library_add = Arc::new(library_add::LibraryAddController::new(
                Arc::clone(&scanner),
                Arc::clone(&scan_findings),
                Arc::clone(&scan_installs),
                Arc::clone(&installation_store),
                install_folders,
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

            let history = Arc::clone(&library);
            if let Err(error) = std::thread::Builder::new()
                .name("dla-history-prune".to_owned())
                .spawn(move || {
                    std::thread::sleep(HISTORY_PRUNE_DELAY);
                    let cutoff = (time::OffsetDateTime::now_utc() - HISTORY_RETENTION)
                        .format(&time::format_description::well_known::Rfc3339);
                    let pruned = cutoff
                        .map_err(|error| error.to_string())
                        .and_then(|cutoff| {
                            history
                                .prune_history(&cutoff)
                                .map_err(|error| error.to_string())
                        });
                    match pruned {
                        Ok(report) => log::info!(
                            target: "dla::lifecycle",
                            "event=history_pruned scan_sessions={} media_sessions={}",
                            report.scan_sessions,
                            report.media_sessions
                        ),
                        Err(error) => log::warn!("library history prune failed: {error}"),
                    }
                })
            {
                log::warn!("library history prune could not start: {error}");
            }
            let thumbnails = ThumbnailCache::new(cache_directory.join("media/thumbnails"));
            let pruned = thumbnails.clone();
            if let Err(error) = std::thread::Builder::new()
                .name("dla-thumbnail-prune".to_owned())
                .spawn(move || {
                    if let Err(error) = pruned.prune(THUMBNAIL_CACHE_LIMIT) {
                        log::warn!("media thumbnail cache prune failed: {error}");
                    }
                })
            {
                log::warn!("media thumbnail cache prune could not start: {error}");
            }
            app.manage(thumbnails);
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
                scan_findings,
                scan_installs,
                #[cfg(desktop)]
                library_add,
                installation_review,
                library_shelves,
                work_preferences,
                own_works,
                local_personalization,
                package_preparation,
                maintenance,
                launch,
                launch_options,
                locale_remulator,
                translation,
                media,
                personalized_voice,
                native_video: Arc::new(native_video::NativeVideoController::default()),
                #[cfg(desktop)]
                delivery,
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
            commands::browse_library_scan_issues,
            commands::list_scan_findings,
            commands::set_scan_finding_ignored,
            commands::add_scan_findings,
            commands::read_scan_installs,
            commands::install_library_archive,
            commands::cancel_scan_install,
            commands::select_install_folder,
            commands::start_library_add,
            commands::read_library_add,
            commands::confirm_library_add,
            commands::cancel_library_add,
            commands::dismiss_library_add,
            commands::list_library_installations,
            commands::read_library_shelves,
            commands::read_work_preference,
            commands::list_work_preferences,
            commands::replace_work_preference,
            commands::read_local_personalization,
            commands::list_library_installations_for_work,
            commands::read_library_installation,
            commands::save_library_installation_review,
            commands::list_own_work_details,
            commands::read_own_work_details,
            commands::save_own_work_details,
            commands::list_installation_images,
            commands::import_own_work_artwork,
            commands::import_own_work_artwork_file,
            commands::select_installation_destination,
            commands::select_installation_location,
            commands::read_delivery,
            commands::delivery_sign_in,
            commands::delivery_sign_out,
            commands::delivery_install,
            commands::delivery_pause,
            commands::delivery_resume,
            commands::delivery_cancel,
            commands::delivery_remove,
            commands::select_delivery_install_folder,
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
            commands::open_installation_folder,
            commands::read_launch_options,
            commands::save_launch_options,
            commands::cleanup_library_maintenance,
            commands::check_library_file_names,
            commands::fix_library_file_names,
            commands::fix_all_library_file_names,
            commands::inspect_package_destination,
            commands::start_package_preparation,
            commands::cancel_package_preparation,
            commands::read_package_preparation_progress,
            commands::launch_library_installation,
            commands::stop_library_launch,
            commands::list_recent_launches,
            commands::list_installation_launch_history,
            translation::read_translation_settings,
            translation::save_translation_settings,
            translation::test_translator,
            translation::read_game_translation,
            translation::turn_on_game_translation,
            translation::show_game_translation,
            translation::remove_game_translation,
            translation::prepare_game_translation,
            translation::translate_game_again,
            translation::pause_game_translation,
            translation::check_translation_pack,
            translation::read_translated_works,
            translation::download_translation_pack,
            commands::open_library_media_session,
            commands::list_library_media_items,
            commands::read_library_audio_waveform,
            commands::open_personalized_voice_queue,
            commands::read_library_media_session,
            commands::update_library_media_progress,
            commands::update_library_media_queue_settings,
            commands::close_library_media_session,
            commands::read_video_backend,
            commands::read_video_subtitles,
            commands::open_native_video,
            commands::update_native_video_viewport,
            commands::control_native_video,
            commands::close_native_video,
            commands::open_video_externally,
            commands::select_media_player,
            commands::play_in_player,
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
            render_recovery::recover_webview_rendering,
            commands::maximize_window,
            support::read_support_status,
            support::acknowledge_unclean_shutdown,
            support::record_frontend_fault,
            support::save_support_bundle,
            support::open_support_issue,
            support::open_support_project,
            read_only_navigation::read_current_read_only_deep_links,
        ])
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            let Some(state) = window.app_handle().try_state::<AppState>() else {
                return;
            };
            let result = match event {
                tauri::WindowEvent::Moved(_)
                | tauri::WindowEvent::Resized(_)
                | tauri::WindowEvent::ScaleFactorChanged { .. } => {
                    state.native_video.reposition_active(window.app_handle())
                }
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    let result = state.native_video.close_all(window.app_handle());
                    window.app_handle().exit(0);
                    result
                }
                tauri::WindowEvent::Destroyed => {
                    state.native_video.close_all(window.app_handle())
                }
                _ => Ok(()),
            };
            if let Err(error) = result {
                log::warn!(target: "dla::media", "event=native_video_window_sync_failed error={error}");
            }
        })
        .build(context)
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

/// SIGINT and SIGTERM end the launcher the way closing the window does, so
/// the shutdown is recorded as clean. A signal handler may only write to a
/// pipe; a thread of its own reads it and asks the app to exit. CEF installs
/// Chromium's handlers while it starts and its runtime puts the earlier
/// disposition back afterwards, so these stay in place on both builds.
#[cfg(target_os = "linux")]
fn install_linux_termination_handlers(app: &tauri::AppHandle) {
    use std::sync::atomic::{AtomicI32, Ordering};

    static WRITE_END: AtomicI32 = AtomicI32::new(-1);
    extern "C" fn on_signal(signal: libc::c_int) {
        let fd = WRITE_END.load(Ordering::Acquire);
        if fd >= 0 {
            let byte = signal as u8;
            // SAFETY: write(2) is async-signal-safe and the pipe stays open for
            // the life of the process.
            unsafe { libc::write(fd, std::ptr::from_ref(&byte).cast(), 1) };
        }
    }

    let mut ends = [0 as libc::c_int; 2];
    // SAFETY: `ends` has room for the two descriptors pipe2 fills in.
    if unsafe { libc::pipe2(ends.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        log::warn!(
            target: "dla::lifecycle",
            "event=termination_handlers_unavailable error={}",
            std::io::Error::last_os_error()
        );
        return;
    }
    WRITE_END.store(ends[1], Ordering::Release);
    for signal in [libc::SIGINT, libc::SIGTERM] {
        // SAFETY: a zeroed sigaction is a valid one to fill in, and the handler
        // only writes to the pipe.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
            action.sa_flags = libc::SA_RESTART;
            libc::sigemptyset(&mut action.sa_mask);
            libc::sigaction(signal, &action, std::ptr::null_mut());
        }
    }

    let app = app.clone();
    let read_end = ends[0];
    let spawned = std::thread::Builder::new()
        .name("dla-termination".to_owned())
        .spawn(move || {
            let mut byte = 0u8;
            loop {
                // SAFETY: `byte` is a one-byte buffer for the pipe's read end.
                let read = unsafe { libc::read(read_end, std::ptr::from_mut(&mut byte).cast(), 1) };
                if read == 1 {
                    break;
                }
                if read < 0
                    && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
                {
                    continue;
                }
                return;
            }
            let signal = i32::from(byte);
            if let Some(support) = app.try_state::<SupportRuntime>() {
                log::info!(
                    target: "dla::lifecycle",
                    "event=termination_requested signal={signal} run_id={}",
                    support.run_id()
                );
                if support.mark_clean_shutdown() {
                    log::info!(target: "dla::lifecycle", "event=clean_shutdown run_id={}", support.run_id());
                }
            }
            let exiting = app.clone();
            let _ = app.run_on_main_thread(move || exiting.exit(0));
        });
    if let Err(error) = spawned {
        log::warn!(target: "dla::lifecycle", "event=termination_handlers_unavailable error={error}");
    }
}

#[cfg(all(target_os = "linux", not(feature = "chromium")))]
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
            #[cfg(all(target_os = "linux", not(feature = "chromium")))]
            {
                use gtk::prelude::{GtkWindowExt, WidgetExt};

                if let Ok(native_window) = main_window.gtk_window() {
                    native_window.show_all();
                    native_window.present();
                }
            }
            #[cfg(any(not(target_os = "linux"), feature = "chromium"))]
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
            // The in-page player's <track> elements load through dla-subtitle.
            assert!(media.contains("dla-subtitle:"));
            assert!(media.contains("http://dla-subtitle.localhost"));
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
