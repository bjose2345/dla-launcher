import { operationIsTerminal } from "../../app/operationStage";
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
export type PackageDestinationConflictPolicy = "refuse" | "keep_both" | "replace_existing";
export type PackageDestinationState =
  | "available"
  | "occupied_unknown"
  | "managed_same_installation"
  | "managed_other_installation"
  | "managed_orphaned_installation";

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
  catalogRelease?: {
    romPosition: number;
    romCount: number;
    name: string;
    version: string;
    updateDate: string;
  } | null;
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

/** A work's file names that an older install read wrongly: Japanese names
 * that turned into symbols. */
export interface FileNameReport {
  installationId: string;
  /** Names still wrong. */
  wrong: number;
  /** Names given back just now. */
  fixed: number;
}

export interface LibraryFileNameReport {
  /** Works whose names were fixed. */
  works: number;
  files: number;
  /** Works left as they were: in use, or a fixed name already taken. */
  skipped: number;
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

/** A part of the page over the video that must stay visible, in CSS pixels. */
export interface NativeVideoRectangle {
  x: number;
  y: number;
  width: number;
  height: number;
  /** Corner radius, for a round control. */
  radius?: number;
}

export interface NativeVideoSubtitle {
  index: number;
  label: string;
  language: string | null;
  format: "web_vtt" | "sub_rip" | "sub_station_alpha" | "sami";
}

export type NativeVideoSubtitleColor = "white" | "yellow" | "cyan";

/** How subtitles are drawn, on every player: the native surfaces and the overlay. */
export interface NativeVideoSubtitleStyle {
  /** Text size as a factor of the normal size, 1 being normal. */
  size: number;
  color: NativeVideoSubtitleColor;
  /** Text opacity, 0 to 1. */
  opacity: number;
  /** Opacity of the black box behind the text, 0 to 1; 0 is no box. */
  background: number;
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
  subtitleStyle: NativeVideoSubtitleStyle;
  /** Whether the letterbox bands carry a blurred copy of the picture. */
  ambient: boolean;
  labels: {
    back: string;
    player: string;
    play: string;
    playlist: string;
    openPlaylist: string;
    closePlaylist: string;
    nowPlaying: string;
    playbackFailed: string;
    codecUnsupported: string;
    exitFullscreen: string;
    retry: string;
  };
}

export interface NativeVideoControlRequest {
  sessionId: string;
  command: "play" | "pause" | "seek" | "volume" | "muted" | "fit" | "retry" | "fullscreen" | "subtitle" | "rate"
    | "subtitle_style" | "ambient";
  value?: number;
  /** A seek while scrubbing: quick, landing on the nearest keyframe. */
  preview?: boolean;
  subtitleStyle?: NativeVideoSubtitleStyle;
  enabled?: boolean;
  fit?: NativeVideoFit;
  subtitleTrack?: number;
  /** The command came from a click on the picture: the player shows the play or pause badge over it. */
  badge?: boolean;
}

export interface NativeVideoState {
  surfaceId: string;
  sessionId: string;
  ordinal: number;
  kind: "loading" | "metadata" | "ready" | "playing" | "paused" | "time" | "waiting" | "ended" | "error" | "interaction" | "fullscreen" | "subtitle" | "subtitle_error" | "back" | "choose" | "volume";
  positionSeconds: number;
  durationSeconds: number | null;
  paused: boolean;
  ended: boolean;
  readyState: number;
  errorCode: number | null;
  subtitleTrack: number | null;
  actionOrdinal: number | null;
  fullscreen: boolean;
  /** The surface's own volume after the user changed it there (kind "volume"). */
  volume?: number | null;
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

export interface LaunchOptions {
  /** Start the work as if Windows were Japanese, through Locale Remulator. */
  japaneseLocale: boolean;
  /** Only Windows offers it: Wine always runs games in Japanese. */
  japaneseLocaleAvailable: boolean;
  /** The Visual C++ runtimes Locale Remulator needs and Windows lacks. */
  missingRuntimes: VisualCppRuntime[];
  /** A video player of the user's to play this work's videos in, or none for the built-in player. */
  videoPlayer: string | null;
  /** An audio player of the user's to play this work's tracks in, or none for the built-in player. */
  audioPlayer: string | null;
  /** How Linux shows the game's window; null leaves it as the game made it. */
  windowScale: WindowScale | null;
  /** Linux with gamescope installed can enlarge a game's window. */
  windowScaleAvailable: boolean;
  /** The refresh rate of the enlarged game in Hz; null is the launcher's default. */
  refreshRate: number | null;
}

export interface LaunchOptionsRequest {
  japaneseLocale: boolean;
  videoPlayer: string | null;
  audioPlayer: string | null;
  windowScale: WindowScale | null;
  refreshRate: number | null;
}

/**
 * A small game's window enlarged through gamescope: full screen fitted, full screen
 * by whole numbers so its pixels stay sharp, or a window of the given size.
 */
export type WindowScale = "fit" | "integer" | `${number}x${number}`;

/** Which of a work's media an external player is for. */
export type MediaPlayerKind = "audio" | "video";

export type VisualCppRuntime = "x64" | "x86";

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
  /** Opens the work's folder in the system file manager, on desktop. */
  openInstallationFolder?(installationId: string): Promise<void>;
  /** How a work starts, on desktop. */
  readLaunchOptions?(installationId: string): Promise<LaunchOptions>;
  saveLaunchOptions?(installationId: string, options: LaunchOptionsRequest): Promise<LaunchOptions>;
  /** Lets the user pick a player program; nothing when they cancel. */
  selectMediaPlayer?(): Promise<string | null>;
  /** Plays the work's videos or tracks in the player chosen for them, the given one first. */
  playInPlayer?(installationId: string, kind: MediaPlayerKind, ordinal: number | null): Promise<void>;
  /** Opens a download the launcher points to, such as Microsoft's runtime. */
  openWebsite?(url: string): Promise<void>;
  cleanupMaintenance(): Promise<MaintenanceCleanupReport>;
  /** Finds and fixes file names an older install read wrongly, on desktop. */
  checkFileNames?(installationId: string): Promise<FileNameReport>;
  fixFileNames?(installationId: string): Promise<FileNameReport>;
  launchInstallation(installationId: string): Promise<LaunchActivity>;
  stopLaunch(activityId: string): Promise<LaunchActivity>;
  listRecentLaunches(limit: number): Promise<LaunchActivity[]>;
  /** Reports a launch each time its stored state changes. */
  subscribeLaunchActivity?(listener: (activity: LaunchActivity) => void): Promise<() => void>;
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
  listOwnWorkDetails(): Promise<OwnWorkDetails[]>;
  readOwnWorkDetails(installationId: string): Promise<OwnWorkDetails | null>;
  saveOwnWorkDetails(request: SaveOwnWorkDetailsRequest): Promise<OwnWorkDetails>;
  listInstallationImages(installationId: string): Promise<InstallationImage[]>;
  /** Copies one of the work's images into the Library's own artwork; returns its id. */
  importOwnWorkArtwork(installationId: string, relativePath: string): Promise<string>;
  /** Asks for a picture file and copies it into the Library's own artwork; null when cancelled. */
  importOwnWorkArtworkFile(installationId: string): Promise<string | null>;
  ownWorkArtworkUrl(installationId: string, artworkId: string): string;
  /** A small copy of one of the work's images. */
  installationImageUrl(installationId: string, relativePath: string): string;
  /** A small copy of an image item, made once and kept by the app. */
  mediaThumbnailUrl(sessionId: string, ordinal: number): string;
  /** The sidecar subtitle files beside a video, for the page's own player. */
  readVideoSubtitles?(sessionId: string, ordinal: number): Promise<NativeVideoSubtitle[]>;
  /** A subtitle file as WebVTT, for a `<track>` of the page's own player. */
  videoSubtitleUrl?(sessionId: string, ordinal: number, trackIndex: number): string;
  openNativeVideo?(request: OpenNativeVideoRequest): Promise<OpenNativeVideoResponse>;
  /**
   * Who draws the player's bar with the native video surface: the surface
   * itself (a webview window), or the page, above a surface that nothing can
   * be drawn over (the Chromium build's GStreamer window). Surface by default.
   */
  nativeVideoChrome?: "surface" | "page";
  /**
   * How the audio element gets its bytes: streamed from the media URL with
   * range requests, or the whole track fetched into a blob first, for a
   * webview that cannot stream the private protocol (WebKitGTK, ADR 0028).
   * Blob by default.
   */
  audioDelivery?: MediaDelivery;
  /**
   * How the reader gets a PDF: "stream" reads the byte ranges the shown pages
   * need; "blob" fetches the whole file first, for a webview that cannot serve
   * ranges from the media protocol. Streaming when absent.
   */
  documentDelivery?: MediaDelivery;
  /** Opens the video in the system's video player, for files the launcher cannot play. */
  openVideoExternally?(sessionId: string, ordinal: number): Promise<void>;
  updateNativeVideoViewport?(
    sessionId: string,
    surfaceId: string,
    viewport: NativeVideoViewport,
    obstructions?: NativeVideoRectangle[],
  ): Promise<void>;
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

export type MediaDelivery = "stream" | "blob";

export function launchActivityIsActive(status: LaunchActivityStatus): boolean {
  return status === "starting" || status === "running" || status === "stopping";
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
  if (operationIsTerminal(current.stage)) return current;
  if (operationIsTerminal(incoming.stage)) return incoming;
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
    && operationIsTerminal(progress.stage);
}

export const manageTabs = ["general", "files", "display", "translation"] as const;
export type ManageTab = (typeof manageTabs)[number];

export type OwnWorkKind = "images" | "manga" | "game" | "voice" | "video" | "document";

/**
 * What the user says about a work the catalog doesn't know, shown where a
 * catalog work shows the catalog's details.
 */
export interface OwnWorkDetails {
  installationId: string;
  circle: string | null;
  code: string | null;
  releaseDate: string | null;
  kind: OwnWorkKind | null;
  tags: string[];
  description: string | null;
  /** Artwork ids in the Library's own artwork. */
  cover: string | null;
  samples: string[];
  updatedAt: string;
}

export type SaveOwnWorkDetailsRequest = Omit<OwnWorkDetails, "updatedAt">;

export interface InstallationImage {
  relativePath: string;
  sizeBytes: number | null;
}

