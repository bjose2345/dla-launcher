import { describe, expect, it } from "vitest";

declare const process: {
  getBuiltinModule(id: "fs"): {
    readdirSync(path: URL, options: { recursive: true }): string[];
  };
};

const readdirSync = process.getBuiltinModule("fs").readdirSync;

describe("TypeScript module names", () => {
  it("remain unambiguous on case-insensitive filesystems", () => {
    const modulePaths = readdirSync(new URL(".", import.meta.url), { recursive: true })
      .filter((path) => /\.(?:ts|tsx)$/.test(path) && !path.endsWith(".d.ts"))
      .map((path) => path.replace(/\.(?:ts|tsx)$/, ""));
    const modulesByPortablePath = new Map<string, string[]>();

    for (const modulePath of modulePaths) {
      const portablePath = modulePath.toLowerCase();
      const existing = modulesByPortablePath.get(portablePath) ?? [];
      existing.push(modulePath);
      modulesByPortablePath.set(portablePath, existing);
    }

    const collisions = [...modulesByPortablePath.values()]
      .filter((paths) => paths.length > 1)
      .map((paths) => paths.sort())
      .sort(([left = ""], [right = ""]) => left.localeCompare(right));

    expect(collisions).toEqual([]);
  });
});
