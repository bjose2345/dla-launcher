import type { CatalogDescriptionVersion } from "./types";

export function orderDescriptionVersions(versions: CatalogDescriptionVersion[]) {
  return [...versions].sort((left, right) => left.version - right.version);
}

export function latestDescriptionVersion(versions: CatalogDescriptionVersion[]) {
  return orderDescriptionVersions(versions).at(-1) ?? null;
}
