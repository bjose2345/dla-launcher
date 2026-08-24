const preReleasePattern = /(?:^|[^a-z])(alpha|beta|rc|dev|next)(?![a-z])/i;

export function isPreReleaseVersion(version: string): boolean {
  return preReleasePattern.test(version);
}
