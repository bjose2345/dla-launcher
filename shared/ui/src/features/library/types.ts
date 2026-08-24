import type { CatalogWork } from "../catalog/types";

export type InstallationPlatform = "windows" | "linux" | "macos" | "android" | "ios" | "unknown";
export type InstallationStatus = "ready" | "needs_review";
export type InferenceConfidence = "low" | "medium" | "high";
export type MediaType = "executable" | "audio" | "image" | "pdf" | "video" | "archive" | "android_package" | "directory" | "unknown";
export type LaunchActionKind = "launch_executable" | "play_audio" | "read_images" | "open_document" | "play_video" | "open_archive" | "open_android_package";
export type LaunchAdapter = "windows_native" | "linux_native" | "linux_wine";

export type LaunchTarget =
  | { kind: "installation_root" }
  | { kind: "relative_path"; path: string };

export type ManualCatalogIdentity =
  | { kind: "catalog_work"; workCode: string }
  | { kind: "unidentified" };

export interface CatalogIdentity {
  workCode: string;
  confidence: "possible" | "strong" | "exact";
  reasonCodes: string[];
}

export interface ContentItem {
  relativePath: string;
  pathKey: string;
  mediaType: MediaType;
  sizeBytes: number | null;
  modifiedAt: string | null;
  confidence: InferenceConfidence;
  reasonCodes: string[];
}

export interface LaunchCandidate {
  id: string;
  action: LaunchActionKind;
  target: LaunchTarget;
  supportedPlatforms: InstallationPlatform[];
  confidence: InferenceConfidence;
  reasonCodes: string[];
}

export type PackageSafety = "safe" | "unsafe";
export type PackageContentKind =
  | "windows_game"
  | "windows_application"
  | "audio_collection"
  | "image_collection"
  | "video_collection"
  | "android_application"
  | "mixed_media"
  | "unknown";

export interface PackageIssue {
  code: string;
  entryIndex: number | null;
  path: string | null;
}

export interface PackageLaunchCandidate {
  action: LaunchActionKind;
  relativePath: string;
  supportedPlatforms: InstallationPlatform[];
  confidence: InferenceConfidence;
  reasonCodes: string[];
  expectedSha256?: string | null;
}

export type ArchiveFormat = "zip" | "rar";
export type PackageSourceSetKind = "single_archive" | "multipart_rar" | "multipart_rar_sfx";
export type ArchiveRetentionPolicy = "keep" | "delete_after_verified_install";
export type PackageDestinationConflictPolicy = "refuse" | "keep_both";
export type PackageDestinationState =
  | "available"
  | "occupied_unknown"
  | "managed_same_installation"
  | "managed_other_installation";

export interface PackageSourceArtifact {
  scanEntryId: string;
  kind: "archive" | "directory";
  relativePath: string;
  sizeBytes: number | null;
  sha256: string | null;
}

export interface PackageSourceSet {
  kind: PackageSourceSetKind;
  volumes: PackageSourceArtifact[];
}

export interface PackageInspection {
  source: {
    scanEntryId: string;
    kind: "archive" | "directory";
    relativePath: string;
    sizeBytes: number | null;
    sha256: string | null;
  };
  sourceSet?: PackageSourceSet | null;
  format: ArchiveFormat;
  safety: PackageSafety;
  entryCount: number;
  fileCount: number;
  directoryCount: number;
  totalCompressedBytes: number;
  totalUncompressedBytes: number;
  commonRoot: string | null;
  issues: PackageIssue[];
  classification: {
    contentKind: PackageContentKind;
    engine: string | null;
    platform: InstallationPlatform;
    confidence: InferenceConfidence;
    reasonCodes: string[];
    contentRoot: string | null;
    launchCandidates: PackageLaunchCandidate[];
  };
  installPlan: {
    requiresExtraction: boolean;
    contentRoot: string | null;
    preferredAction: PackageLaunchCandidate | null;
    archiveRetention: ArchiveRetentionPolicy;
  };
  inspectedAt: string;
}

export type PackagePreparationStage =
  | "queued"
  | "validating"
  | "extracting"
  | "verifying"
  | "activating"
  | "cleaning_sources"
  | "completed"
  | "cancelled"
  | "failed";

export interface PackagePreparationCounters {
  totalBytes: number;
  processedBytes: number;
  totalFiles: number;
  processedFiles: number;
}

export interface PackagePreparationProgress {
  operationId: string;
  installationId: string;
  stage: PackagePreparationStage;
  counters: PackagePreparationCounters;
  currentPath: string | null;
  detail: string;
}

export interface PreparedPackageInstallation {
  installationId: string;
  destinationRoot: string;
  contentRoot: string | null;
  preferredAction: PackageLaunchCandidate | null;
  sourceSet: PackageSourceSet;
  archiveRetention: ArchiveRetentionPolicy;
  sourcesDeleted: boolean;
  sourceCleanupError: string | null;
  installedFileCount: number;
  installedBytes: number;
  preparedAt: string;
}

export interface SelectedInstallationDestination {
  accessHandle: string;
  displayPath: string;
}

export interface PackageDestinationPreview {
  state: PackageDestinationState;
  destinationName: string;
  keepBothDestinationName: string | null;
}

export type InstallationHealthState =
  | "unknown"
  | "healthy"
  | "missing_files"
  | "modified_files"
  | "moved"
  | "inaccessible"
  | "needs_review"
  | "repairable";

export type InstallationHealthIssueKind =
  | "missing"
  | "modified"
  | "inaccessible"
  | "unexpected"
  | "invalid_ownership_marker";

export interface InstallationHealthIssue {
  kind: InstallationHealthIssueKind;
  relativePath: string | null;
  detail: string;
}

export interface InstallationHealthReport {
  installationId: string;
  state: InstallationHealthState;
  managed: boolean;
  repairable: boolean;
  checkedRoot: string;
  expectedFiles: number;
  presentFiles: number;
  missingFiles: number;
  modifiedFiles: number;
  inaccessibleFiles: number;
  unexpectedFiles: number;
  issues: InstallationHealthIssue[];
  checkedAt: string;
}

export interface MaintenanceCleanupReport {
  removedStagingDirectories: number;
  removedRepairDirectories: number;
  restoredSourceFiles: number;
  retainedPaths: string[];
}

export type LaunchActivityStatus =
  | "starting"
  | "running"
  | "stopping"
  | "exited"
  | "failed"
  | "stopped"
  | "interrupted";

export interface LaunchActivity {
  id: string;
  installationId: string;
  action: LaunchActionKind | null;
  targetPath: string | null;
  adapter: LaunchAdapter | null;
  status: LaunchActivityStatus;
  processId: number | null;
  error: string | null;
  attemptedAt: string;
  startedAt: string | null;
  endedAt: string | null;
  durationMs: number | null;
  exitCode: number | null;
  stopRequestedAt: string | null;
}

export type MediaSessionStatus =
  | "active"
  | "paused"
  | "completed"
  | "closed"
  | "interrupted"
  | "failed";
export type MediaSessionKind = "work" | "personalized_voice";
export type MediaRepeatMode = "off" | "all" | "one";

export interface MediaSessionItem {
  ordinal: number;
  installationId: string;
  workCode: string | null;
  relativePath: string;
  mediaType: MediaType;
  sizeBytes: number | null;
  discNumber: number | null;
  trackNumber: number | null;
  bonus: boolean;
}

export interface AudioWaveform {
  peaks: number[];
  durationMs: number;
}

export interface MediaProgress {
  itemOrdinal: number;
  positionMs: number;
  durationMs: number | null;
  completed: boolean;
  updatedAt: string;
}

export interface MediaSession {
  id: string;
  kind: MediaSessionKind;
  installationId: string;
  action: LaunchActionKind;
  status: MediaSessionStatus;
  repeatMode: MediaRepeatMode;
  shuffle: boolean;
  items: MediaSessionItem[];
  progress: MediaProgress;
  openedAt: string;
  updatedAt: string;
  endedAt: string | null;
  error: string | null;
}

export type LibraryActivityKind = "executable_launch" | "media_session";

export interface LibraryRecentActivity {
  installationId: string;
  action: LaunchActionKind | null;
  kind: LibraryActivityKind;
  occurredAt: string;
  active: boolean;
}

export interface MediaResume {
  installationId: string;
  action: LaunchActionKind;
  relativePath: string;
  positionMs: number;
  durationMs: number | null;
  completed: boolean;
  updatedAt: string;
}

export interface LibraryLaunchTotals {
  installationId: string;
  launchCount: number;
  totalDurationMs: number;
}

export interface LibraryShelves {
  installations: Installation[];
  recent: LibraryRecentActivity[];
  continueItems: MediaResume[];
  neverLaunched: string[];
  unfinished: MediaResume[];
  launchTotals: LibraryLaunchTotals[];
}

export type WorkPreferenceKind = "favorite" | "not_interested";

export interface WorkPreference {
  workCode: string;
  preference: WorkPreferenceKind;
  updatedAt: string;
}

export interface PersonalizationAnchor {
  workCode: string;
  title: string;
  action: LaunchActionKind;
}

export interface PersonalizedRecommendationItem {
  work: CatalogWork;
  score: number;
  anchors: PersonalizationAnchor[];
}

export interface LocalPersonalization {
  favorites: CatalogWork[];
  becauseYou: PersonalizedRecommendationItem[];
  voiceMix: PersonalizedRecommendationItem[];
  activityWorkCount: number;
  voiceActivityWorkCount: number;
  becauseYouMinimum: number;
  voiceMixMinimum: number;
}

export interface UpdateMediaProgressRequest {
  sessionId: string;
  itemOrdinal: number;
  positionMs: number;
  durationMs: number | null;
  completed: boolean;
  status: Extract<MediaSessionStatus, "active" | "paused" | "completed">;
}

export interface UpdateMediaQueueSettingsRequest {
  sessionId: string;
  repeatMode: MediaRepeatMode;
  shuffle: boolean;
}

export type NativeVideoFit = "contain" | "cover" | "original";

export interface NativeVideoViewport {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface NativeVideoSubtitle {
  index: number;
  label: string;
  language: string | null;
  format: "web_vtt" | "sub_rip" | "sub_station_alpha" | "sami";
}

export interface NativeVideoPlaylistItem {
  ordinal: number;
  itemName: string;
}

export interface OpenNativeVideoResponse {
  surfaceId: string;
  subtitles: NativeVideoSubtitle[];
}

export interface OpenNativeVideoRequest {
  sessionId: string;
  ordinal: number;
  viewport: NativeVideoViewport;
  positionSeconds: number;
  volume: number;
  muted: boolean;
  fit: NativeVideoFit;
  autoPlay: boolean;
  posterUrl: string | null;
  title: string;
  itemName: string;
  completed: boolean;
  playlist: NativeVideoPlaylistItem[];
  subtitleLabel: string | null;
  labels: {
    back: string;
    player: string;
    play: string;
    markFinished: string;
    completed: string;
    playlist: string;
    openPlaylist: string;
    closePlaylist: string;
    nowPlaying: string;
    playbackFailed: string;
    codecUnsupported: string;
    retry: string;
  };
}

export interface NativeVideoControlRequest {
  sessionId: string;
  command: "play" | "pause" | "seek" | "volume" | "muted" | "fit" | "retry" | "fullscreen" | "subtitle";
  value?: number;
  enabled?: boolean;
  fit?: NativeVideoFit;
  subtitleTrack?: number;
}

export interface NativeVideoState {
  surfaceId: string;
  sessionId: string;
  ordinal: number;
  kind: "loading" | "metadata" | "ready" | "playing" | "paused" | "time" | "waiting" | "ended" | "error" | "interaction" | "fullscreen" | "subtitle" | "subtitle_error" | "back" | "finish" | "choose";
  positionSeconds: number;
  durationSeconds: number | null;
  paused: boolean;
  ended: boolean;
  readyState: number;
  errorCode: number | null;
  subtitleTrack: number | null;
  actionOrdinal: number | null;
  fullscreen: boolean;
}

export interface ManualLaunchSelection {
  action: LaunchActionKind;
  target: LaunchTarget;
}

export interface ContentItemOverride {
  relativePath: string;
  mediaType: MediaType | null;
  ignored: boolean;
  order: number | null;
}

export interface InstallationOverrides {
  catalogIdentity: ManualCatalogIdentity | null;
  customTitle: string | null;
  preferredAction: ManualLaunchSelection | null;
  contentItems: ContentItemOverride[];
  reviewedAt: string | null;
}

export interface Installation {
  id: string;
  scanRootId: string | null;
  rootPath: string;
  platform: InstallationPlatform;
  status: InstallationStatus;
  detection: {
    sourceScanSessionId: string | null;
    catalogIdentity: CatalogIdentity | null;
    suggestedStatus: InstallationStatus;
    contentItems: ContentItem[];
    launchCandidates: LaunchCandidate[];
    packageInspection: PackageInspection | null;
  };
  overrides: InstallationOverrides;
  discoveredAt: string;
  updatedAt: string;
}

export interface InstallationReviewRequest {
  installationId: string;
  catalogIdentity: ManualCatalogIdentity | null;
  customTitle: string | null;
  preferredAction: ManualLaunchSelection | null;
  contentItems: ContentItemOverride[];
}

export interface LibraryGateway {
  readShelves(): Promise<LibraryShelves>;
  readWorkPreference(workCode: string): Promise<WorkPreference | null>;
  listWorkPreferences(): Promise<WorkPreference[]>;
  replaceWorkPreference(
    workCode: string,
    preference: WorkPreferenceKind | null,
  ): Promise<WorkPreference | null>;
  readLocalPersonalization(): Promise<LocalPersonalization>;
  listInstallations(): Promise<Installation[]>;
  listInstallationsForWork(workCode: string): Promise<Installation[]>;
  readInstallation(installationId: string): Promise<Installation>;
  saveReview(request: InstallationReviewRequest): Promise<Installation>;
  selectInstallationDestination(): Promise<SelectedInstallationDestination | null>;
  inspectPackageDestination(
    installationId: string,
    destinationAccessHandle: string,
  ): Promise<PackageDestinationPreview>;
  selectInstallationLocation(): Promise<SelectedInstallationDestination | null>;
  readPreparedPackage(installationId: string): Promise<PreparedPackageInstallation | null>;
  readPreparedPackages(installationIds: string[]): Promise<PreparedPackageInstallation[]>;
  readInstallationHealth(installationId: string): Promise<InstallationHealthReport>;
  readInstallationHealths(installationIds: string[]): Promise<InstallationHealthReport[]>;
  verifyInstallation(installationId: string): Promise<InstallationHealthReport>;
  locateInstallation(
    installationId: string,
    locationAccessHandle: string,
  ): Promise<InstallationHealthReport>;
  rescanInstallation(installationId: string): Promise<InstallationHealthReport>;
  repairInstallation(installationId: string): Promise<InstallationHealthReport>;
  removeInstallation(installationId: string): Promise<void>;
  uninstallInstallation(installationId: string): Promise<void>;
  cleanupMaintenance(): Promise<MaintenanceCleanupReport>;
  launchInstallation(installationId: string): Promise<LaunchActivity>;
  stopLaunch(activityId: string): Promise<LaunchActivity>;
  listRecentLaunches(limit: number): Promise<LaunchActivity[]>;
  listInstallationLaunchHistory(
    installationId: string,
    limit: number,
  ): Promise<LaunchActivity[]>;
  listMediaItems(installationId: string): Promise<MediaSessionItem[]>;
  readAudioWaveform(
    installationId: string,
    ordinal: number,
    bucketCount: number,
  ): Promise<AudioWaveform>;
  openMediaSession(installationId: string): Promise<MediaSession>;
  openPersonalizedVoiceQueue(): Promise<MediaSession>;
  readMediaSession(sessionId: string): Promise<MediaSession>;
  updateMediaProgress(request: UpdateMediaProgressRequest): Promise<MediaSession>;
  updateMediaQueueSettings(request: UpdateMediaQueueSettingsRequest): Promise<MediaSession>;
  closeMediaSession(sessionId: string): Promise<MediaSession>;
  mediaAssetUrl(sessionId: string, ordinal: number): string;
  openNativeVideo?(request: OpenNativeVideoRequest): Promise<OpenNativeVideoResponse>;
  updateNativeVideoViewport?(sessionId: string, surfaceId: string, viewport: NativeVideoViewport): Promise<void>;
  controlNativeVideo?(request: NativeVideoControlRequest): Promise<void>;
  closeNativeVideo?(sessionId: string, surfaceId: string): Promise<void>;
  subscribeNativeVideoState?(
    listener: (state: NativeVideoState) => void,
  ): Promise<() => void>;
  startPackagePreparation(
    installationId: string,
    destinationAccessHandle: string,
    destinationConflictPolicy: PackageDestinationConflictPolicy,
    archiveRetention: ArchiveRetentionPolicy,
  ): Promise<PackagePreparationProgress>;
  cancelPackagePreparation(operationId: string): Promise<boolean>;
  readPackagePreparationProgress(): Promise<PackagePreparationProgress | null>;
  subscribePackagePreparationProgress(
    listener: (progress: PackagePreparationProgress) => void,
  ): Promise<() => void>;
}

export function launchActivityIsActive(status: LaunchActivityStatus): boolean {
  return status === "starting" || status === "running" || status === "stopping";
}

export function packagePreparationIsTerminal(stage: PackagePreparationStage | undefined): boolean {
  return stage === "completed" || stage === "cancelled" || stage === "failed";
}

export function packagePreparationIsIndeterminate(stage: PackagePreparationStage | undefined): boolean {
  return stage === "queued"
    || stage === "validating"
    || stage === "extracting"
    || stage === "activating"
    || stage === "cleaning_sources";
}

export function packagePreparationCanCancel(stage: PackagePreparationStage | undefined): boolean {
  return stage === "queued"
    || stage === "validating"
    || stage === "extracting"
    || stage === "verifying"
    || stage === "activating";
}

const packagePreparationStageOrder: Record<PackagePreparationStage, number> = {
  queued: 0,
  validating: 1,
  extracting: 2,
  verifying: 3,
  activating: 4,
  cleaning_sources: 5,
  completed: 6,
  cancelled: 6,
  failed: 6,
};

export function mergePackagePreparationProgress(
  current: PackagePreparationProgress | null | undefined,
  incoming: PackagePreparationProgress | null,
): PackagePreparationProgress | null {
  if (!incoming) return current ?? null;
  if (!current || current.operationId !== incoming.operationId) return incoming;
  if (packagePreparationIsTerminal(current.stage)) return current;
  if (packagePreparationIsTerminal(incoming.stage)) return incoming;
  const currentStage = packagePreparationStageOrder[current.stage];
  const incomingStage = packagePreparationStageOrder[incoming.stage];
  if (incomingStage < currentStage) return current;
  if (
    incomingStage === currentStage
    && (
      incoming.counters.processedBytes < current.counters.processedBytes
      || incoming.counters.processedFiles < current.counters.processedFiles
    )
  ) return current;
  return incoming;
}

export function packagePreparationNeedsPreparedRefresh(
  progress: PackagePreparationProgress | null | undefined,
  installationId: string,
): boolean {
  return progress?.installationId === installationId
    && packagePreparationIsTerminal(progress.stage);
}
