/** The last segment of a path, whichever separator it uses; a path without one is its own name. */
export function basename(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
}
