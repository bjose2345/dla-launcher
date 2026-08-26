import type { CatalogWork } from "../catalog";
import { effectiveIdentity, installationTitle } from "./libraryPresentation";
import type {
  Installation,
  LaunchActionKind,
  LibraryLaunchTotals,
  LibraryShelves,
  MediaResume,
} from "./types";

export type LibraryContentKind = "audio" | "images" | "video" | "documents" | "apps" | "other";

export const libraryLenses = ["all", "audio", "apps", "images", "video", "documents"] as const;

export type LibraryLens = (typeof libraryLenses)[number];

export const libraryViews = ["shelves", "grid", "list"] as const;

export type LibraryView = (typeof libraryViews)[number];

export function libraryLensCounts(kinds: LibraryContentKind[]): Map<LibraryLens, number> {
  const counts = new Map<LibraryLens, number>([["all", kinds.length]]);
  for (const kind of kinds) {
    if (kind === "other") continue;
    counts.set(kind, (counts.get(kind) ?? 0) + 1);
  }
  return counts;
}

export function matchesLibraryLens(lens: LibraryLens, kind: LibraryContentKind): boolean {
  return lens === "all" || lens === kind;
}

export function launchTotalsByInstallation(
  totals: LibraryLaunchTotals[],
): Map<string, LibraryLaunchTotals> {
  return new Map(totals.map((entry) => [entry.installationId, entry]));
}

export function playTimeParts(totalDurationMs: number): { hours: number; minutes: number } {
  const totalMinutes = Math.max(0, Math.floor(totalDurationMs / 60_000));
  return { hours: Math.floor(totalMinutes / 60), minutes: totalMinutes % 60 };
}

const actionKinds: Partial<Record<LaunchActionKind, LibraryContentKind>> = {
  launch_executable: "apps",
  play_audio: "audio",
  read_images: "images",
  open_document: "documents",
  play_video: "video",
  open_android_package: "apps",
};

export function libraryContentKind(
  installation: Installation,
  action: LaunchActionKind | null,
): LibraryContentKind {
  if (action && actionKinds[action]) return actionKinds[action];

  const packageKind = installation.detection.packageInspection?.classification.contentKind;
  if (packageKind === "audio_collection") return "audio";
  if (packageKind === "image_collection") return "images";
  if (packageKind === "video_collection") return "video";
  if (
    packageKind === "windows_game"
    || packageKind === "windows_application"
    || packageKind === "android_application"
  ) return "apps";

  const counts = new Map<LibraryContentKind, number>();
  for (const item of installation.detection.contentItems) {
    const kind = item.mediaType === "audio"
      ? "audio"
      : item.mediaType === "image"
        ? "images"
        : item.mediaType === "video"
          ? "video"
          : item.mediaType === "pdf"
            ? "documents"
            : item.mediaType === "executable" || item.mediaType === "android_package"
              ? "apps"
              : null;
    if (kind) counts.set(kind, (counts.get(kind) ?? 0) + 1);
  }

  const priority: LibraryContentKind[] = ["audio", "images", "video", "documents", "apps"];
  return priority.reduce<LibraryContentKind>((selected, candidate) => (
    (counts.get(candidate) ?? 0) > (counts.get(selected) ?? 0) ? candidate : selected
  ), "other");
}

export function featuredInstallationId(
  shelves: LibraryShelves,
  eligibleInstallationIds?: ReadonlySet<string>,
): string | null {
  const eligible = (installationId: string) => (
    eligibleInstallationIds === undefined || eligibleInstallationIds.has(installationId)
  );
  const newestActivity = [
    ...shelves.continueItems.map((item) => ({
      installationId: item.installationId,
      occurredAt: item.updatedAt,
      recentLaunch: false,
    })),
    ...shelves.recent.map((item) => ({
      installationId: item.installationId,
      occurredAt: item.occurredAt,
      recentLaunch: true,
    })),
  ].filter((item) => eligible(item.installationId)).reduce<{
    installationId: string;
    occurredAt: string;
    recentLaunch: boolean;
  } | null>((selected, candidate) => {
    if (!selected) return candidate;
    const selectedTime = Date.parse(selected.occurredAt);
    const candidateTime = Date.parse(candidate.occurredAt);
    if (!Number.isFinite(selectedTime)) return candidate;
    if (!Number.isFinite(candidateTime)) return selected;
    if (candidateTime !== selectedTime) return candidateTime > selectedTime ? candidate : selected;
    return candidate.recentLaunch && !selected.recentLaunch ? candidate : selected;
  }, null);

  return newestActivity?.installationId
    ?? shelves.neverLaunched.find(eligible)
    ?? shelves.installations.find((item) => eligible(item.id))?.id
    ?? null;
}

export function resumeForInstallation(
  shelves: LibraryShelves,
  installationId: string,
): MediaResume | null {
  return shelves.continueItems.find((resume) => resume.installationId === installationId)
    ?? shelves.unfinished.find((resume) => resume.installationId === installationId)
    ?? null;
}

export function libraryDisplayTitle(
  installation: Installation,
  work: CatalogWork | undefined,
  preferEnglish: boolean,
): string {
  if (!work) return installationTitle(installation);
  return preferEnglish && work.titleEnglish.trim() ? work.titleEnglish : work.title;
}

export function libraryDisplayCreator(
  installation: Installation,
  work: CatalogWork | undefined,
  preferEnglish: boolean,
): string {
  const circle = work?.circles[0];
  if (circle) return preferEnglish && circle.nameEnglish.trim() ? circle.nameEnglish : circle.name;
  return effectiveIdentity(installation) ?? installation.rootPath;
}
