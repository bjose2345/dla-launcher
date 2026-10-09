import type { Installation } from "./types";

export function installationTitle(installation: Installation): string {
  const customTitle = installation.overrides.customTitle?.trim();
  if (customTitle) return customTitle;
  const identity = effectiveIdentity(installation);
  if (identity) return identity;
  return installationName(installation);
}

/**
 * The name of the work's own files. A work scanned inside a folder of
 * several works shares that folder as its root, so its own top-level folder,
 * archive or file names it better than the root does.
 */
export function installationName(installation: Installation): string {
  const archive = installation.detection.packageInspection?.source.relativePath;
  if (archive) return withoutExtension(lastSegment(archive));
  const paths = installation.detection.contentItems.map((item) => item.relativePath);
  const tops = new Set(paths.map((path) => path.split("/")[0]));
  const [top] = tops;
  if (tops.size === 1 && top) {
    if (paths.every((path) => path.includes("/"))) return top;
    if (paths.length === 1) return withoutExtension(top);
  }
  return lastSegment(installation.rootPath) || installation.rootPath;
}

/**
 * The folder that holds the work's own files: its top-level folder inside a
 * scan root shared with other works, or the root itself.
 */
export function installationFolder(installation: Installation): string {
  const top = ownTopFolder(installation);
  if (!top) return installation.rootPath;
  const separator = installation.rootPath.includes("\\") ? "\\" : "/";
  return `${installation.rootPath.replace(/[\\/]+$/, "")}${separator}${top}`;
}

/** A content path as seen from installationFolder(). */
export function pathInInstallationFolder(installation: Installation, relativePath: string): string {
  const top = ownTopFolder(installation);
  return top && relativePath.startsWith(`${top}/`) ? relativePath.slice(top.length + 1) : relativePath;
}

function ownTopFolder(installation: Installation): string | null {
  if (installation.detection.packageInspection) return null;
  const paths = installation.detection.contentItems.map((item) => item.relativePath);
  const tops = new Set(paths.map((path) => path.split("/")[0]));
  const [top] = tops;
  if (tops.size !== 1 || !top || !paths.every((path) => path.includes("/"))) return null;
  return top;
}

export function effectiveIdentity(installation: Installation): string | null {
  const manual = installation.overrides.catalogIdentity;
  if (manual?.kind === "catalog_work") return manual.workCode;
  if (manual?.kind === "unidentified") return null;
  return installation.detection.catalogIdentity?.workCode ?? null;
}

function lastSegment(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).at(-1) ?? "";
}

function withoutExtension(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(0, dot) : name;
}
