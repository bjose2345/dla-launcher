import { describe, expect, it } from "vitest";

import { localizedName } from "./localizedNames";

describe("localizedName", () => {
  it("prefers the English name only when asked and when there is one", () => {
    expect(localizedName("サークル", "Circle", true)).toBe("Circle");
    expect(localizedName("サークル", "Circle", false)).toBe("サークル");
    expect(localizedName("サークル", "  ", true)).toBe("サークル");
    expect(localizedName("サークル", "", true)).toBe("サークル");
  });
});
