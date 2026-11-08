import { describe, expect, it } from "vitest";

import { untranslatableLanguage } from "./language";

describe("untranslatableLanguage", () => {
  it("translates Japanese works, and works whose languages are not known", () => {
    expect(untranslatableLanguage(["JPN"])).toBeNull();
    expect(untranslatableLanguage(["JPN", "CHI_HANS", "KO_KR"])).toBeNull();
    expect(untranslatableLanguage([])).toBeNull();
    expect(untranslatableLanguage(undefined)).toBeNull();
  });

  it("says why it leaves works in English, or not in Japanese, alone", () => {
    expect(untranslatableLanguage(["JPN", "ENG"])).toBe("includes_english");
    expect(untranslatableLanguage(["ENG"])).toBe("includes_english");
    expect(untranslatableLanguage(["CHI_HANT"])).toBe("not_japanese");
    expect(untranslatableLanguage(["KO_KR"])).toBe("not_japanese");
  });

  it("reads the language names the DLA catalog uses", () => {
    expect(untranslatableLanguage(["japanese"])).toBeNull();
    expect(untranslatableLanguage(["japanese", "chinese_simplified", "korean_south_korea"])).toBeNull();
    expect(untranslatableLanguage(["chinese", "english", "japanese"])).toBe("includes_english");
    expect(untranslatableLanguage(["chinese_traditional"])).toBe("not_japanese");
  });
});
