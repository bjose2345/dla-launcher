import { describe, expect, it } from "vitest";

import { latestDescriptionVersion, orderDescriptionVersions } from "./descriptionVersions";

describe("work description versions", () => {
  const versions = [
    { version: 2, html: "<p>Second</p>" },
    { version: 1, html: "<p>First</p>" },
    { version: 4, html: "<p>Latest</p>" },
  ];

  it("orders versions without mutating the catalog projection", () => {
    expect(orderDescriptionVersions(versions).map((entry) => entry.version)).toEqual([1, 2, 4]);
    expect(versions.map((entry) => entry.version)).toEqual([2, 1, 4]);
  });

  it("selects the highest imported version as latest", () => {
    expect(latestDescriptionVersion(versions)?.version).toBe(4);
    expect(latestDescriptionVersion([])).toBeNull();
  });
});
