import { describe, expect, it } from "vitest";

import { summarizeChecks } from "./report";

describe("shared diagnostics summary", () => {
  it("counts passing and failing capability checks", () => {
    const summary = summarizeChecks([
      { key: "open", label: "Open", passed: true, detail: "ok" },
      { key: "reopen", label: "Reopen", passed: false, detail: "failed" },
      { key: "fts", label: "FTS", passed: true, detail: "ok" },
    ]);

    expect(summary).toEqual({ passed: 2, failed: 1, total: 3 });
  });
});
