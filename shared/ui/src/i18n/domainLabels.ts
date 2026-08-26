import type { MessageKey } from "./catalogs";

import type {
  ArchiveRetentionPolicy,
  InferenceConfidence,
  InstallationPlatform,
  LaunchActionKind,
  LaunchAdapter,
  MediaType,
  PackageContentKind,
  PackageSourceSetKind,
} from "../features/library/types";
import type {
  CatalogGenerationKind,
  CatalogPackageProfile,
} from "../features/importer/types";

const platformKeys = {
  windows: "domain.platform.windows",
  linux: "domain.platform.linux",
  macos: "domain.platform.macos",
  android: "domain.platform.android",
  ios: "domain.platform.ios",
  unknown: "domain.platform.unknown",
} satisfies Record<InstallationPlatform, MessageKey>;

const confidenceKeys: Record<string, MessageKey> = {
  low: "domain.confidence.low",
  medium: "domain.confidence.medium",
  high: "domain.confidence.high",
  possible: "domain.confidence.possible",
  strong: "domain.confidence.strong",
  exact: "domain.confidence.exact",
};

const mediaTypeKeys = {
  executable: "domain.media.executable",
  audio: "domain.media.audio",
  image: "domain.media.image",
  pdf: "domain.media.pdf",
  video: "domain.media.video",
  archive: "domain.media.archive",
  android_package: "domain.media.androidPackage",
  directory: "domain.media.directory",
  unknown: "domain.media.unknown",
} satisfies Record<MediaType, MessageKey>;

const launchActionKeys = {
  launch_executable: "domain.action.launchExecutable",
  play_audio: "domain.action.playAudio",
  read_images: "domain.action.readImages",
  open_document: "domain.action.openDocument",
  play_video: "domain.action.playVideo",
  open_archive: "domain.action.openArchive",
  open_android_package: "domain.action.openAndroidPackage",
} satisfies Record<LaunchActionKind, MessageKey>;

const launchAdapterKeys = {
  windows_native: "domain.adapter.windowsNative",
  linux_native: "domain.adapter.linuxNative",
  linux_wine: "domain.adapter.linuxWine",
} satisfies Record<LaunchAdapter, MessageKey>;

const packageContentKeys = {
  windows_game: "domain.package.windowsGame",
  windows_application: "domain.package.windowsApplication",
  audio_collection: "domain.package.audioCollection",
  image_collection: "domain.package.imageCollection",
  video_collection: "domain.package.videoCollection",
  android_application: "domain.package.androidApplication",
  mixed_media: "domain.package.mixedMedia",
  unknown: "domain.package.unknown",
} satisfies Record<PackageContentKind, MessageKey>;

const sourceSetKeys = {
  single_archive: "domain.sourceSet.singleArchive",
  multipart_rar: "domain.sourceSet.multipartRar",
  multipart_rar_sfx: "domain.sourceSet.multipartRarSfx",
} satisfies Record<PackageSourceSetKind, MessageKey>;

const archivePolicyKeys = {
  keep: "domain.archivePolicy.keep",
  delete_after_verified_install: "domain.archivePolicy.deleteAfterVerifiedInstall",
} satisfies Record<ArchiveRetentionPolicy, MessageKey>;

const catalogProfileKeys = {
  compact: "domain.profile.compact",
  full: "domain.profile.full",
  custom: "domain.profile.custom",
} satisfies Record<CatalogPackageProfile, MessageKey>;

const generationKindKeys = {
  embedded: "domain.generation.embedded",
  imported: "domain.generation.imported",
} satisfies Record<CatalogGenerationKind, MessageKey>;

const evidenceReasonKeys: Record<string, MessageKey> = {
  code_in_directory_name: "domain.evidence.codeInName",
  code_in_filename: "domain.evidence.codeInName",
  code_in_path: "domain.evidence.codeInName",
  filename_observed: "domain.evidence.codeInName",
  archive_md5_match: "domain.evidence.archiveHashMatch",
  archive_sha1_match: "domain.evidence.archiveHashMatch",
  archive_sha256_match: "domain.evidence.archiveHashMatch",
  archive_md5_mismatch: "domain.evidence.archiveHashMismatch",
  archive_sha1_mismatch: "domain.evidence.archiveHashMismatch",
  archive_sha256_mismatch: "domain.evidence.archiveHashMismatch",
  android_package_extension: "domain.evidence.fileType",
  archive_extension: "domain.evidence.fileType",
  audio_extension: "domain.evidence.fileType",
  executable_extension: "domain.evidence.fileType",
  file_extension: "domain.evidence.fileType",
  image_extension: "domain.evidence.fileType",
  pdf_extension: "domain.evidence.fileType",
  video_extension: "domain.evidence.fileType",
  missing_extension: "domain.evidence.fileType",
  unrecognized_extension: "domain.evidence.fileType",
  unsupported_document_extension: "domain.evidence.fileType",
  catalog_application_format: "domain.evidence.catalogFormat",
  catalog_game_category: "domain.evidence.catalogCategory",
  catalog_internal_manifest_match: "domain.evidence.catalogManifest",
  android_package_candidate: "domain.evidence.launchCandidate",
  executable_candidate: "domain.evidence.launchCandidate",
  executable_launch_candidate: "domain.evidence.launchCandidate",
  no_safe_launch_candidate: "domain.evidence.launchCandidate",
  preferred_executable_name: "domain.evidence.preferredExecutable",
  conventional_game_executable: "domain.evidence.preferredExecutable",
  windows_executable: "domain.evidence.preferredExecutable",
  dominant_audio_set: "domain.evidence.mediaCollection",
  dominant_image_set: "domain.evidence.mediaCollection",
  numbered_filename: "domain.evidence.numberedSequence",
  numbered_image_sequence: "domain.evidence.numberedSequence",
  numbered_track_sequence: "domain.evidence.numberedSequence",
  nwjs_package_manifest: "domain.evidence.packageLayout",
  rpg_maker_layout: "domain.evidence.packageLayout",
  rpg_maker_system_manifest: "domain.evidence.packageLayout",
  archive_symlink: "domain.evidence.archiveSafety",
  case_colliding_archive_path: "domain.evidence.archiveSafety",
  deceptive_double_extension: "domain.evidence.archiveSafety",
  duplicate_archive_path: "domain.evidence.archiveSafety",
  encrypted_archive_entry: "domain.evidence.archiveSafety",
  signature_required: "domain.evidence.archiveSafety",
  unsafe_archive_manifest: "domain.evidence.archiveSafety",
  unsafe_archive_path: "domain.evidence.archiveSafety",
  cover_filename: "domain.evidence.ignoredFile",
  known_ignored_filename: "domain.evidence.ignoredFile",
  installer_deprioritized: "domain.evidence.installer",
  installer_name: "domain.evidence.installer",
  single_android_package: "domain.evidence.singleItem",
  single_archive: "domain.evidence.singleItem",
  single_audio: "domain.evidence.singleItem",
  single_executable: "domain.evidence.singleItem",
  single_pdf: "domain.evidence.singleItem",
  single_video: "domain.evidence.singleItem",
  pdf_document: "domain.evidence.singleItem",
  video_file: "domain.evidence.singleItem",
  verified_package_action: "domain.evidence.verifiedAction",
  fixture: "domain.evidence.observed",
};

export function platformMessageKey(value: string): MessageKey {
  return platformKeys[value as InstallationPlatform] ?? platformKeys.unknown;
}

export function confidenceMessageKey(value: string): MessageKey {
  return confidenceKeys[value] ?? "domain.confidence.unknown";
}

export function mediaTypeMessageKey(value: MediaType): MessageKey {
  return mediaTypeKeys[value];
}

export function launchActionMessageKey(value: LaunchActionKind): MessageKey {
  return launchActionKeys[value];
}

export function launchAdapterMessageKey(value: LaunchAdapter): MessageKey {
  return launchAdapterKeys[value];
}

export function packageContentMessageKey(value: PackageContentKind): MessageKey {
  return packageContentKeys[value];
}

export function sourceSetMessageKey(value: PackageSourceSetKind): MessageKey {
  return sourceSetKeys[value];
}

export function archivePolicyMessageKey(value: ArchiveRetentionPolicy): MessageKey {
  return archivePolicyKeys[value];
}

export function catalogProfileMessageKey(value: CatalogPackageProfile): MessageKey {
  return catalogProfileKeys[value];
}

export function generationKindMessageKey(value: CatalogGenerationKind): MessageKey {
  return generationKindKeys[value];
}

export function evidenceReasonMessageKey(value: string): MessageKey {
  return evidenceReasonKeys[value] ?? "domain.evidence.observed";
}
