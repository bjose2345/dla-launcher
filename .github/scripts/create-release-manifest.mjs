import { createHash } from "node:crypto";
import { readFile, readdir, stat, writeFile } from "node:fs/promises";
import path from "node:path";

const [assetsDirectory, outputPath, tag, publicBaseUrl, sourceCommit] =
  process.argv.slice(2);

if (!assetsDirectory || !outputPath || !tag || !publicBaseUrl || !sourceCommit) {
  throw new Error(
    "Usage: create-release-manifest.mjs <assets-dir> <output> <tag> <public-base-url> <source-commit>",
  );
}

const semverTag =
  /^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/;
const tagMatch = semverTag.exec(tag);
if (!tagMatch) throw new Error(`Invalid release tag: ${tag}`);
if (!/^[0-9a-f]{40}$/i.test(sourceCommit)) {
  throw new Error(`Invalid source commit: ${sourceCommit}`);
}

const baseUrl = new URL(publicBaseUrl);
if (baseUrl.protocol !== "https:") {
  throw new Error("The public release base URL must use HTTPS.");
}
const basePath = baseUrl.pathname.replace(/\/+$/, "");
baseUrl.search = "";
baseUrl.hash = "";

const definitions = [
  {
    id: "windows-nsis",
    kind: "installer",
    platform: "windows",
    format: "nsis",
    architecture: "x86_64",
    matches: (name) => name.endsWith(".exe"),
  },
  {
    id: "windows-msi",
    kind: "installer",
    platform: "windows",
    format: "msi",
    architecture: "x86_64",
    matches: (name) => name.endsWith(".msi"),
  },
  {
    id: "linux-deb",
    kind: "installer",
    platform: "linux",
    format: "deb",
    architecture: "x86_64",
    matches: (name) => name.endsWith(".deb"),
  },
  {
    id: "linux-rpm",
    kind: "installer",
    platform: "linux",
    format: "rpm",
    architecture: "x86_64",
    matches: (name) => name.endsWith(".rpm"),
  },
  {
    id: "source-zip",
    kind: "source",
    platform: "source",
    format: "zip",
    architecture: null,
    matches: (name) => name.endsWith("_source.zip"),
  },
  {
    id: "source-tar-gz",
    kind: "source",
    platform: "source",
    format: "tar.gz",
    architecture: null,
    matches: (name) => name.endsWith("_source.tar.gz"),
  },
  {
    id: "checksums",
    kind: "checksums",
    platform: "all",
    format: "sha256",
    architecture: null,
    matches: (name) => name === "SHA256SUMS.txt",
  },
];

const names = (await readdir(assetsDirectory)).sort();
if (names.some((name) => !/^[A-Za-z0-9][A-Za-z0-9._+-]{0,199}$/.test(name))) {
  throw new Error("Release asset names must contain only safe URL characters.");
}

const selected = definitions.map((definition) => {
  const matches = names.filter(definition.matches);
  if (matches.length !== 1) {
    throw new Error(
      `Expected exactly one ${definition.id} asset, found ${matches.length}.`,
    );
  }
  return { definition, name: matches[0] };
});

if (selected.length !== names.length) {
  const selectedNames = new Set(selected.map(({ name }) => name));
  const unexpected = names.filter((name) => !selectedNames.has(name));
  throw new Error(`Unexpected release assets: ${unexpected.join(", ")}`);
}

async function sha256(filePath) {
  return createHash("sha256").update(await readFile(filePath)).digest("hex");
}

const checksumPath = path.join(assetsDirectory, "SHA256SUMS.txt");
const checksumLines = (await readFile(checksumPath, "utf8"))
  .split(/\r?\n/)
  .filter(Boolean);
const checksumEntries = new Map();
for (const line of checksumLines) {
  const match = /^([0-9a-f]{64})  ([A-Za-z0-9][A-Za-z0-9._+-]{0,199})$/.exec(
    line,
  );
  if (!match || checksumEntries.has(match[2])) {
    throw new Error(`Invalid checksum line: ${line}`);
  }
  checksumEntries.set(match[2], match[1]);
}

for (const name of names.filter((name) => name !== "SHA256SUMS.txt")) {
  const actual = await sha256(path.join(assetsDirectory, name));
  if (checksumEntries.get(name) !== actual) {
    throw new Error(`Checksum mismatch or missing checksum for ${name}.`);
  }
}
if (checksumEntries.size !== names.length - 1) {
  throw new Error("SHA256SUMS.txt contains unexpected entries.");
}

const releasePath = `releases/${encodeURIComponent(tag)}`;
const assets = [];
for (const { definition, name } of selected) {
  const filePath = path.join(assetsDirectory, name);
  const fileStat = await stat(filePath);
  assets.push({
    id: definition.id,
    kind: definition.kind,
    platform: definition.platform,
    format: definition.format,
    architecture: definition.architecture,
    name,
    size: fileStat.size,
    sha256: await sha256(filePath),
    url: `${baseUrl.origin}${basePath}/${releasePath}/${encodeURIComponent(name)}`,
  });
}

const manifest = {
  schemaVersion: 1,
  version: tag.slice(1),
  tag,
  prerelease: Boolean(tagMatch[4]),
  publishedAt: new Date().toISOString(),
  sourceCommit: sourceCommit.toLowerCase(),
  assets,
};

await writeFile(outputPath, `${JSON.stringify(manifest, null, 2)}\n`, "utf8");
