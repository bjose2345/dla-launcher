import { describe, expect, it } from "vitest";

import {
  assignBinding,
  ariaKeyShortcuts,
  bindingActions,
  bindingConflicts,
  bindingsForAction,
  conflictsWith,
  eventCombo,
  formatCombo,
  resetBinding,
  resolveBindings,
  sanitizeOverrides,
} from "./keyBindings";

describe("key bindings", () => {
  it("falls back to code defaults for anything the user never touched", () => {
    const resolved = resolveBindings({ toggleSidebar: ["ctrl+d"] });

    expect(bindingsForAction(resolved, "toggleSidebar")).toEqual(["ctrl+d"]);
    expect(bindingsForAction(resolved, "search")).toEqual(["ctrl+k"]);
  });

  it("keeps every action addressable", () => {
    const resolved = resolveBindings({});
    for (const action of bindingActions) {
      expect(bindingsForAction(resolved, action.id).length).toBeGreaterThan(0);
    }
  });

  it("formats a combo for display", () => {
    expect(formatCombo("ctrl+k")).toBe("Ctrl K");
    expect(formatCombo("arrowleft")).toBe("←");
    expect(formatCombo("space")).toBe("Space");
    expect(formatCombo("plus")).toBe("+");
    expect(formatCombo("ctrl+shift+p")).toBe("Ctrl Shift P");
  });

  it("formats every configured shortcut for assistive technology", () => {
    expect(ariaKeyShortcuts(["ctrl+b", "alt+arrowleft"]))
      .toBe("Control+B Meta+B Alt+ArrowLeft");
    expect(ariaKeyShortcuts([])).toBeUndefined();
  });

  it("captures the plus key without producing an unparseable combo", () => {
    expect(eventCombo({
      key: "+",
      ctrlKey: false,
      metaKey: false,
      altKey: false,
      shiftKey: true,
    } as KeyboardEvent)).toBe("plus");
  });

  it("only reports a conflict inside a scope or against a global binding", () => {
    const resolved = resolveBindings({});

    expect(conflictsWith(resolved, "toggleSidebar", "ctrl+k")).toBe("search");
    expect(conflictsWith(resolved, "videoPlayPause", "arrowleft")).toBe("videoSkipBack");
    expect(conflictsWith(resolved, "videoPlayPause", "arrowright")).toBe("videoSkipForward");
  });

  it("treats playback as always live, so it conflicts across scopes", () => {
    const resolved = resolveBindings({});

    expect(conflictsWith(resolved, "videoPlayPause", "ctrl+space")).toBe("playPause");
    expect(conflictsWith(resolved, "playPause", "space")).toBe("videoPlayPause");
  });

  it("lets the video player and the reader share a key", () => {
    const resolved = resolveBindings({});

    expect(bindingsForAction(resolved, "videoSkipBack")).toContain("arrowleft");
    expect(bindingsForAction(resolved, "readerPreviousPage")).toContain("arrowleft");
    expect(conflictsWith(resolved, "readerPreviousPage", "arrowleft")).toBeNull();
  });

  it("takes a key from whichever action already held it", () => {
    const resolved = resolveBindings({});
    const next = assignBinding({}, resolved, "toggleSidebar", 0, "ctrl+k");
    const after = resolveBindings(next);

    expect(bindingsForAction(after, "toggleSidebar")).toEqual(["ctrl+k"]);
    expect(bindingsForAction(after, "search")).toEqual([]);
  });

  it("takes a global key from every overlapping scope", () => {
    const resolved = resolveBindings({});
    expect(bindingConflicts(resolved, "toggleSidebar", "arrowleft").map((action) => action.id))
      .toEqual(["videoSkipBack", "readerPreviousPage"]);

    const after = resolveBindings(assignBinding({}, resolved, "toggleSidebar", 0, "arrowleft"));
    expect(bindingsForAction(after, "toggleSidebar")).toEqual(["arrowleft"]);
    expect(bindingsForAction(after, "videoSkipBack")).toEqual(["j"]);
    expect(bindingsForAction(after, "readerPreviousPage")).toEqual([]);
  });

  it("does not steal a shortcut from a fixed action", () => {
    const resolved = resolveBindings({});
    const overrides = assignBinding({}, resolved, "openSettings", 0, "escape");

    expect(overrides).toEqual({});
    expect(bindingsForAction(resolveBindings(overrides), "openSettings")).toEqual(["ctrl+,"]);
    expect(bindingsForAction(resolveBindings(overrides), "readerClose")).toEqual(["escape"]);
  });

  it("repairs conflicting persisted overrides before dispatch", () => {
    const resolved = resolveBindings({ toggleSidebar: ["arrowleft"] });

    expect(bindingsForAction(resolved, "toggleSidebar")).toEqual(["arrowleft"]);
    expect(bindingsForAction(resolved, "videoSkipBack")).toEqual(["j"]);
    expect(bindingsForAction(resolved, "readerPreviousPage")).toEqual([]);
  });

  it("does not duplicate a shortcut inside one action", () => {
    const resolved = resolveBindings({});
    const overrides = assignBinding({}, resolved, "videoPlayPause", 1, "space");

    expect(overrides).toEqual({});
  });

  it("restores a default by dropping the override", () => {
    const overrides = assignBinding({}, resolveBindings({}), "search", 0, "ctrl+j");
    const restored = resetBinding(overrides, "search");

    expect(bindingsForAction(resolveBindings(restored), "search")).toEqual(["ctrl+k"]);
  });

  it("refuses to override a fixed action", () => {
    const overrides = assignBinding({}, resolveBindings({}), "readerClose", 0, "q");

    expect(bindingsForAction(resolveBindings(overrides), "readerClose")).toEqual(["escape"]);
  });

  it("discards stored entries that no longer name a real action", () => {
    expect(sanitizeOverrides({ search: ["ctrl+j"], removedAction: ["x"], readerClose: ["q"] }))
      .toEqual({ search: ["ctrl+j"] });
  });

  it("survives a corrupt store", () => {
    expect(sanitizeOverrides(null)).toEqual({});
    expect(sanitizeOverrides("nonsense")).toEqual({});
    expect(sanitizeOverrides({ search: "ctrl+j" })).toEqual({});
  });

  it("normalizes, deduplicates, and migrates stored combos", () => {
    expect(sanitizeOverrides({
      search: ["CTRL+J", "ctrl+j", "ctrl+", 42],
      readerZoomIn: ["+", "="],
      toggleSidebar: ["ctrl+b"],
    })).toEqual({
      search: ["ctrl+j"],
    });
  });
});
