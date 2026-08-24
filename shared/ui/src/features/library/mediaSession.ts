import type {
  Installation,
  LaunchActionKind,
  MediaSession,
  MediaSessionItem,
  PreparedPackageInstallation,
} from "./types";

const mediaActions = new Set<LaunchActionKind>([
  "play_audio",
  "read_images",
  "open_document",
  "play_video",
]);

export function isMediaLaunchAction(action: LaunchActionKind): boolean {
  return mediaActions.has(action);
}

export function installationMediaAction(
  installation: Installation,
  prepared: PreparedPackageInstallation | null | undefined,
): LaunchActionKind | null {
  const action = installationPrimaryAction(installation, prepared);
  return action && isMediaLaunchAction(action) ? action : null;
}

export function installationPrimaryAction(
  installation: Installation,
  prepared: PreparedPackageInstallation | null | undefined,
): LaunchActionKind | null {
  if (installation.status !== "ready" || installation.overrides.reviewedAt === null) return null;
  return prepared?.preferredAction?.action
    ?? (installation.detection.packageInspection === null
      ? installation.overrides.preferredAction?.action
      : null)
    ?? null;
}

export function mediaActionMessageKey(action: LaunchActionKind) {
  switch (action) {
    case "play_audio": return "media.action.listen" as const;
    case "read_images": return "media.action.read" as const;
    case "play_video": return "media.action.watch" as const;
    default: return "media.action.open" as const;
  }
}

export function mediaSessionTitleMessageKey(session: MediaSession) {
  switch (session.action) {
    case "play_audio": return "media.player.audio" as const;
    case "read_images": return "media.player.images" as const;
    case "open_document": return "media.player.document" as const;
    case "play_video": return "media.player.video" as const;
    default: return "media.player.generic" as const;
  }
}

export function mediaStatusMessageKey(status: MediaSession["status"]) {
  switch (status) {
    case "active": return "media.status.active" as const;
    case "paused": return "media.status.paused" as const;
    case "completed": return "media.status.completed" as const;
    case "closed": return "media.status.closed" as const;
    case "interrupted": return "media.status.interrupted" as const;
    case "failed": return "media.status.failed" as const;
  }
}

export function mediaItemName(item: MediaSessionItem): string {
  return item.relativePath.split(/[\\/]/).filter(Boolean).at(-1) ?? item.relativePath;
}

export function mediaProgressPercent(session: MediaSession): number | null {
  const { durationMs, positionMs } = session.progress;
  if (durationMs !== null && durationMs > 0) {
    return Math.max(0, Math.min(100, (positionMs / durationMs) * 100));
  }
  if (session.action !== "read_images" && session.action !== "open_document") return null;
  if (session.progress.completed) return 100;
  const index = session.items.findIndex((item) => item.ordinal === session.progress.itemOrdinal);
  if (index < 0 || session.items.length === 0) return 0;
  return Math.max(0, Math.min(100, (index / session.items.length) * 100));
}

export function orderedSessionItems(
  session: MediaSession,
  shuffle = session.shuffle,
): MediaSessionItem[] {
  const byOrdinal = [...session.items].sort((left, right) => left.ordinal - right.ordinal);
  if (!shuffle) return byOrdinal;
  let state = stableHash(session.id) || 0x9e3779b9;
  for (let index = byOrdinal.length - 1; index > 0; index -= 1) {
    state ^= state << 13;
    state ^= state >>> 17;
    state ^= state << 5;
    const swapIndex = (state >>> 0) % (index + 1);
    const current = byOrdinal[index]!;
    byOrdinal[index] = byOrdinal[swapIndex]!;
    byOrdinal[swapIndex] = current;
  }
  return byOrdinal;
}

function stableHash(value: string): number {
  let hash = 2166136261;
  for (const character of value) {
    hash ^= character.codePointAt(0) ?? 0;
    hash = Math.imul(hash, 16777619);
  }
  return hash >>> 0;
}
