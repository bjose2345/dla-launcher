import { describe, expect, it } from "vitest";

declare const process: {
  getBuiltinModule(name: "fs"): {
    readFileSync(path: URL, encoding: "utf8"): string;
  };
};

// vite/client types are not wired into this package, so the glob helper is declared locally.
type GlobImporter = (
  pattern: string,
  options: { query: string; import: string; eager: true },
) => Record<string, string>;

const stylesheetFiles = (import.meta as unknown as { glob: GlobImporter })
  .glob("./*.css", { query: "?url", import: "default", eager: true });
const libraryComponents = (import.meta as unknown as { glob: GlobImporter })
  .glob("../features/library/*.tsx", { query: "?raw", import: "default", eager: true });
const readFileSync = process.getBuiltinModule("fs").readFileSync;
const sources = Object.fromEntries(Object.keys(stylesheetFiles).map((name) => [
  name,
  readFileSync(new URL(name, import.meta.url), "utf8"),
]));

type Node =
  | { kind: "other"; raw: string }
  | { kind: "at" | "rule"; selector: string; body: string };

function parse(text: string): Node[] {
  const nodes: Node[] = [];
  let index = 0;
  while (index < text.length) {
    while (index < text.length && /\s/.test(text[index]!)) index += 1;
    if (index >= text.length) break;
    if (text.startsWith("/*", index)) {
      const close = text.indexOf("*/", index);
      const end = close === -1 ? text.length : close + 2;
      nodes.push({ kind: "other", raw: text.slice(index, end) });
      index = end;
      continue;
    }
    const brace = text.indexOf("{", index);
    const semicolon = text.indexOf(";", index);
    if (brace === -1) {
      nodes.push({ kind: "other", raw: text.slice(index).trim() });
      break;
    }
    if (semicolon !== -1 && semicolon < brace) {
      nodes.push({ kind: "other", raw: text.slice(index, semicolon + 1) });
      index = semicolon + 1;
      continue;
    }
    const selector = text.slice(index, brace).trim();
    let depth = 1;
    let cursor = brace + 1;
    while (cursor < text.length && depth > 0) {
      if (text[cursor] === "{") depth += 1;
      else if (text[cursor] === "}") depth -= 1;
      cursor += 1;
    }
    if (depth > 0) {
      nodes.push({ kind: "other", raw: text.slice(index) });
      break;
    }
    nodes.push({
      kind: selector.startsWith("@") ? "at" : "rule",
      selector,
      body: text.slice(brace + 1, cursor - 1),
    });
    index = cursor;
  }
  return nodes;
}

function withoutComments(text: string): string {
  return text.replace(/\/\*[\s\S]*?\*\//g, "");
}

function literalLibraryClassNames(source: string): string[] {
  const found = new Set<string>();
  const add = (value: string) => {
    for (const match of value.matchAll(/\blibrary-[a-z0-9-]*[a-z0-9]\b/g)) {
      found.add(match[0]);
    }
  };
  const className = /\bclassName\s*=\s*(?:"([^"]*)"|'([^']*)'|\{([\s\S]*?)\})/g;
  for (const match of source.matchAll(className)) {
    add(match[1] ?? match[2] ?? match[3] ?? "");
  }
  return [...found];
}

function problems(source: string): string[] {
  const text = withoutComments(source);
  const found: string[] = [];

  // A single selector never spans a newline in hand-written CSS; when it does, two
  // rules have been spliced together without a comma. That is what a botched
  // selector-group edit produces, and brace counting does not notice it.
  for (const match of text.matchAll(/(?:^|\})\s*([^{}@]+)\{/g)) {
    for (const part of (match[1] ?? "").split(",")) {
      if (part.trim().includes("\n")) {
        found.push(`selectors spliced without a comma: ${part.trim().slice(0, 60)}`);
      }
    }
  }
  const walk = (nodes: Node[]) => {
    for (const node of nodes) {
      if (node.kind === "other") {
        const raw = node.raw.trim();
        if (raw && !raw.startsWith("/*") && !raw.startsWith("@")) {
          found.push(`selector fragment with no block: ${raw.slice(0, 60)}`);
        }
        continue;
      }
      if (!node.selector.trim()) found.push("rule with an empty selector");
      if (node.selector.trim().endsWith(",")) found.push(`dangling comma: ${node.selector.slice(0, 60)}`);
      if (node.kind === "at" && /^@(media|supports|container|layer)\b/.test(node.selector)) {
        if (!node.body.trim()) found.push(`empty at-rule: ${node.selector.slice(0, 60)}`);
        else walk(parse(node.body));
      }
    }
  };
  walk(parse(text));
  return found;
}

const stylesheets = Object.keys(sources).sort();

describe("stylesheets", () => {
  it("ships at least one stylesheet to check", () => {
    expect(stylesheets.length).toBeGreaterThan(0);
  });

  it("styles every literal Library component class", () => {
    const css = Object.values(sources).join("\n");
    const classes = Object.entries(libraryComponents)
      .filter(([name]) => !name.endsWith(".test.tsx"))
      .flatMap(([, source]) => literalLibraryClassNames(source));
    const missing = [...new Set(classes)]
      .filter((className) => !new RegExp(`\\.${className}(?![a-z0-9-])`).test(css))
      .sort();

    expect(missing).toEqual([]);
  });

  it.each(stylesheets)("%s parses into complete rules", (name: string) => {
    expect(problems(sources[name] ?? "")).toEqual([]);
  });

  it.each(stylesheets)("%s has balanced braces and no empty rules", (name: string) => {
    const text = sources[name] ?? "";
    const open = (text.match(/\{/g) ?? []).length;
    const close = (text.match(/\}/g) ?? []).length;
    expect({ name, open, close }).toEqual({ name, open: close, close });
    expect(text.match(/\{\s*\}/g) ?? []).toEqual([]);
  });

  it("removes global navigation chrome only while the video player is mounted", () => {
    const video = sources["./video-player.css"] ?? "";

    expect(video).toContain(".app-frame:has(.video-player-page) > .app-topbar");
    expect(video).toContain(".app-frame:has(.video-player-page) .app-sidebar");
    expect(video).not.toContain(".app-frame:has(.media-dock) > .app-topbar");
    expect(video).not.toContain(".app-frame:has(.media-dock) .app-sidebar");
  });

  it("catches the splice that brace counting misses", () => {
    // Reproduces the exact corruption a bad selector-group edit produced: a rule's
    // closing brace glued to the next selector list, with a rule silently swallowed.
    const spliced = [
      ".a svg {",
      "  width: 16px;",
      "}.b,",
      ".c,",
      ".d svg ",
      "",
      ".e {",
      "  display: grid;",
      "}",
    ].join("\n");

    expect(spliced.match(/\{/g)?.length).toBe(spliced.match(/\}/g)?.length);
    expect(problems(spliced).length).toBeGreaterThan(0);
  });

  it("accepts a normal multi-line selector list", () => {
    expect(problems(".a,\n.b,\n.c {\n  color: red;\n}\n")).toEqual([]);
  });
});
