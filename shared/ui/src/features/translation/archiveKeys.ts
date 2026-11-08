/**
 * What the Archive offers depends on the account signed in to DLA, so these
 * queries are asked again when it changes. Kept apart, without imports, so
 * the downloads feature can use them too.
 */
export const archiveTranslationKey = ["translation", "archive"] as const;
export const archivePackKey = (installationId: string) =>
  [...archiveTranslationKey, "pack", installationId] as const;
export const translatedWorksKey = [...archiveTranslationKey, "works"] as const;
