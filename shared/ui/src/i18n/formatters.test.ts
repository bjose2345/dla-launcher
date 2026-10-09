import { describe, expect, it } from "vitest";

import { dateTimeFormatter, formatDateTime, listFormatter, numberFormatter } from "./formatters";

describe("locale formatters", () => {
  it("reuses one formatter per locale", () => {
    expect(dateTimeFormatter("en-US")).toBe(dateTimeFormatter("en-US"));
    expect(numberFormatter("ja-JP")).toBe(numberFormatter("ja-JP"));
    expect(listFormatter("de-DE")).toBe(listFormatter("de-DE"));
    expect(dateTimeFormatter("en-US")).not.toBe(dateTimeFormatter("de-DE"));
  });

  it("preserves malformed timestamps instead of throwing", () => {
    expect(formatDateTime("not-a-date", "en-US")).toBe("not-a-date");
  });

  it("formats a timestamp as a medium date and short time", () => {
    expect(formatDateTime("2026-10-08T18:17:23Z", "en-US")).toMatch(/^Oct 8, 2026, 6:17\s?PM$/);
  });
});
