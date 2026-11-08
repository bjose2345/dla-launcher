import { describe, expect, it } from "vitest";

import type { MessageKey } from "./catalogs";
import { pluralText } from "./pluralText";

const t = (key: MessageKey, values: Record<string, string | number> = {}) => (
  `${key}(${Object.entries(values).map(([name, value]) => `${name}=${value}`).join(",")})`
);

describe("pluralText", () => {
  it("picks the one-key only for exactly one and writes the count for the locale", () => {
    expect(pluralText(t, "en-US", 1, "catalog.work", "catalog.works")).toBe("catalog.work(count=1)");
    expect(pluralText(t, "en-US", 0, "catalog.work", "catalog.works")).toBe("catalog.works(count=0)");
    expect(pluralText(t, "en-US", 1234, "catalog.work", "catalog.works", { size: "2 GiB" }))
      .toBe("catalog.works(size=2 GiB,count=1,234)");
    expect(pluralText(t, "de-DE", 1234, "catalog.work", "catalog.works")).toBe("catalog.works(count=1.234)");
  });
});
