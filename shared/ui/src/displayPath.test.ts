import { describe, expect, it } from "vitest";

import { displayPath } from "./displayPath";

describe("displayPath", () => {
  it("drops the Windows extended-length marker", () => {
    expect(displayPath("\\\\?\\C:\\Users\\develop\\Documents\\DLA Library\\RJ01678657"))
      .toBe("C:\\Users\\develop\\Documents\\DLA Library\\RJ01678657");
    expect(displayPath("\\\\?\\UNC\\server\\share\\works")).toBe("\\\\server\\share\\works");
  });

  it("keeps ordinary paths", () => {
    expect(displayPath("/home/developer/My Works")).toBe("/home/developer/My Works");
    expect(displayPath("D:\\Games")).toBe("D:\\Games");
  });
});
