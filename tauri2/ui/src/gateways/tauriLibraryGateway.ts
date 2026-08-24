import type {
  ArchiveRetentionPolicy,
  AudioWaveform,
  Installation,
  InstallationHealthReport,
  InstallationReviewRequest,
  LaunchActivity,
  LibraryGateway,
  LibraryShelves,
  LocalPersonalization,
  MaintenanceCleanupReport,
  MediaSession,
  MediaSessionItem,
  NativeVideoControlRequest,
  NativeVideoState,
  OpenNativeVideoResponse,
  NativeVideoViewport,
  OpenNativeVideoRequest,
  PackageDestinationConflictPolicy,
  PackageDestinationPreview,
  PackagePreparationProgress,
  PreparedPackageInstallation,
  SelectedInstallationDestination,
  UpdateMediaProgressRequest,
  UpdateMediaQueueSettingsRequest,
  WorkPreference,
  WorkPreferenceKind,
} from "@dla-launcher/shared-ui/library";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { cacheCatalogWork } from "./catalogArtwork";

export const tauriLibraryGateway: LibraryGateway = {
  readShelves(): Promise<LibraryShelves> {
    return invoke("read_library_shelves");
  },
  readWorkPreference(workCode: string): Promise<WorkPreference | null> {
    return invoke("read_work_preference", { workCode });
  },
  listWorkPreferences(): Promise<WorkPreference[]> {
    return invoke("list_work_preferences");
  },
  replaceWorkPreference(
    workCode: string,
    preference: WorkPreferenceKind | null,
  ): Promise<WorkPreference | null> {
    return invoke("replace_work_preference", { workCode, preference });
  },
  async readLocalPersonalization(): Promise<LocalPersonalization> {
    const personalization = await invoke<LocalPersonalization>("read_local_personalization");
    return {
      ...personalization,
      favorites: personalization.favorites.map(cacheCatalogWork),
      becauseYou: personalization.becauseYou.map((item) => ({
        ...item,
        work: cacheCatalogWork(item.work),
      })),
      voiceMix: personalization.voiceMix.map((item) => ({
        ...item,
        work: cacheCatalogWork(item.work),
      })),
    };
  },
  listInstallations(): Promise<Installation[]> {
    return invoke("list_library_installations");
  },
  listInstallationsForWork(workCode: string): Promise<Installation[]> {
    return invoke("list_library_installations_for_work", { workCode });
  },
  readInstallation(installationId: string): Promise<Installation> {
    return invoke("read_library_installation", { installationId });
  },
  saveReview(request: InstallationReviewRequest): Promise<Installation> {
    return invoke("save_library_installation_review", { request });
  },
  selectInstallationDestination(): Promise<SelectedInstallationDestination | null> {
    return invoke("select_installation_destination");
  },
  inspectPackageDestination(
    installationId: string,
    destinationAccessHandle: string,
  ): Promise<PackageDestinationPreview> {
    return invoke("inspect_package_destination", { installationId, destinationAccessHandle });
  },
  selectInstallationLocation(): Promise<SelectedInstallationDestination | null> {
    return invoke("select_installation_location");
  },
  readPreparedPackage(installationId: string): Promise<PreparedPackageInstallation | null> {
    return invoke("read_prepared_package", { installationId });
  },
  readPreparedPackages(installationIds: string[]): Promise<PreparedPackageInstallation[]> {
    return invoke("read_prepared_packages", { installationIds });
  },
  readInstallationHealth(installationId: string): Promise<InstallationHealthReport> {
    return invoke("read_installation_health", { installationId });
  },
  readInstallationHealths(installationIds: string[]): Promise<InstallationHealthReport[]> {
    return invoke("read_installation_healths", { installationIds });
  },
  verifyInstallation(installationId: string): Promise<InstallationHealthReport> {
    return invoke("verify_library_installation", { installationId });
  },
  locateInstallation(
    installationId: string,
    locationAccessHandle: string,
  ): Promise<InstallationHealthReport> {
    return invoke("locate_library_installation", { installationId, locationAccessHandle });
  },
  rescanInstallation(installationId: string): Promise<InstallationHealthReport> {
    return invoke("rescan_library_installation", { installationId });
  },
  repairInstallation(installationId: string): Promise<InstallationHealthReport> {
    return invoke("repair_library_installation", { installationId });
  },
  removeInstallation(installationId: string): Promise<void> {
    return invoke("remove_library_installation", { installationId });
  },
  uninstallInstallation(installationId: string): Promise<void> {
    return invoke("uninstall_library_installation", { installationId });
  },
  cleanupMaintenance(): Promise<MaintenanceCleanupReport> {
    return invoke("cleanup_library_maintenance");
  },
  launchInstallation(installationId: string): Promise<LaunchActivity> {
    return invoke("launch_library_installation", { installationId });
  },
  stopLaunch(activityId: string): Promise<LaunchActivity> {
    return invoke("stop_library_launch", { activityId });
  },
  listRecentLaunches(limit: number): Promise<LaunchActivity[]> {
    return invoke("list_recent_launches", { limit });
  },
  listInstallationLaunchHistory(
    installationId: string,
    limit: number,
  ): Promise<LaunchActivity[]> {
    return invoke("list_installation_launch_history", { installationId, limit });
  },
  listMediaItems(installationId: string): Promise<MediaSessionItem[]> {
    return invoke("list_library_media_items", { installationId });
  },
  readAudioWaveform(
    installationId: string,
    ordinal: number,
    bucketCount: number,
  ): Promise<AudioWaveform> {
    return invoke("read_library_audio_waveform", {
      installationId,
      ordinal,
      bucketCount,
    });
  },
  openMediaSession(installationId: string): Promise<MediaSession> {
    return invoke("open_library_media_session", { installationId });
  },
  openPersonalizedVoiceQueue(): Promise<MediaSession> {
    return invoke("open_personalized_voice_queue");
  },
  readMediaSession(sessionId: string): Promise<MediaSession> {
    return invoke("read_library_media_session", { sessionId });
  },
  updateMediaProgress(request: UpdateMediaProgressRequest): Promise<MediaSession> {
    return invoke("update_library_media_progress", { request });
  },
  updateMediaQueueSettings(request: UpdateMediaQueueSettingsRequest): Promise<MediaSession> {
    return invoke("update_library_media_queue_settings", { request });
  },
  closeMediaSession(sessionId: string): Promise<MediaSession> {
    return invoke("close_library_media_session", { sessionId });
  },
  mediaAssetUrl(sessionId: string, ordinal: number): string {
    return convertFileSrc(`${sessionId}/${ordinal}`, "dla-media");
  },
  openNativeVideo(request: OpenNativeVideoRequest): Promise<OpenNativeVideoResponse> {
    return invoke("open_native_video", { request });
  },
  updateNativeVideoViewport(sessionId: string, surfaceId: string, viewport: NativeVideoViewport): Promise<void> {
    return invoke("update_native_video_viewport", { sessionId, surfaceId, viewport });
  },
  controlNativeVideo(request: NativeVideoControlRequest): Promise<void> {
    return invoke("control_native_video", { request });
  },
  closeNativeVideo(sessionId: string, surfaceId: string): Promise<void> {
    return invoke("close_native_video", { sessionId, surfaceId });
  },
  subscribeNativeVideoState(listener: (state: NativeVideoState) => void): Promise<() => void> {
    return listen<NativeVideoState>("native-video-state", (event) => listener(event.payload));
  },
  startPackagePreparation(
    installationId: string,
    destinationAccessHandle: string,
    destinationConflictPolicy: PackageDestinationConflictPolicy,
    archiveRetention: ArchiveRetentionPolicy,
  ): Promise<PackagePreparationProgress> {
    return invoke("start_package_preparation", {
      installationId,
      destinationAccessHandle,
      destinationConflictPolicy,
      archiveRetention,
    });
  },
  cancelPackagePreparation(operationId: string): Promise<boolean> {
    return invoke("cancel_package_preparation", { operationId });
  },
  readPackagePreparationProgress(): Promise<PackagePreparationProgress | null> {
    return invoke("read_package_preparation_progress");
  },
  subscribePackagePreparationProgress(listener): Promise<() => void> {
    return listen<PackagePreparationProgress>("package-preparation-progress", (event) => {
      listener(event.payload);
    });
  },
};
