import type { Installation } from "./types";

export function installationTitle(installation: Installation): string {
  const customTitle = installation.overrides.customTitle?.trim();
  if (customTitle) return customTitle;
  const identity = effectiveIdentity(installation);
  if (identity) return identity;
  return installation.rootPath.split(/[\\/]/).filter(Boolean).at(-1) ?? installation.rootPath;
}

export function effectiveIdentity(installation: Installation): string | null {
  const manual = installation.overrides.catalogIdentity;
  if (manual?.kind === "catalog_work") return manual.workCode;
  if (manual?.kind === "unidentified") return null;
  return installation.detection.catalogIdentity?.workCode ?? null;
}
