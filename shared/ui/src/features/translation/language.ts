// The DLA catalog names languages ("japanese", "english"); older catalogs
// used codes ("JPN", "ENG").
const JAPANESE = new Set(["japanese", "jpn"]);
const ENGLISH = new Set(["english", "eng"]);

/** Why the launcher has nothing to translate in a work. */
export type UntranslatableLanguage = "includes_english" | "not_japanese";

/**
 * Why the launcher cannot translate a work, from the catalog's languages, or
 * null when it can. It translates Japanese into English only: a work that
 * already includes English, or is not in Japanese (Chinese or Korean only),
 * has nothing for it to do. Languages the catalog does not know leave it open.
 */
export function untranslatableLanguage(
  languageCodes: readonly string[] | undefined,
): UntranslatableLanguage | null {
  if (!languageCodes?.length) return null;
  const languages = languageCodes.map((code) => code.toLowerCase());
  if (languages.some((code) => ENGLISH.has(code))) return "includes_english";
  if (!languages.some((code) => JAPANESE.has(code))) return "not_japanese";
  return null;
}
