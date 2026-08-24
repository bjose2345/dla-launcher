import { useQuery } from "@tanstack/react-query";
import {
  Check,
  ChevronRight,
  CircleAlert,
  Copy,
  EyeOff,
  File,
  FileText,
  Film,
  Folder,
  Image as ImageIcon,
  Music,
  Search,
  X,
} from "lucide-react";
import { useMemo, useRef, useState, type ComponentType, type SVGProps } from "react";

import { usePresentation } from "../../preferences/PresentationProvider";
import type { CatalogDetailGateway, CatalogRomEntry } from "./types";

type HashMode = "off" | "crc32" | "md5" | "sha1" | "sha256";

interface TreeNode {
  id: string;
  name: string;
  children?: TreeNode[];
  entry?: CatalogRomEntry;
}

interface VisibleNode {
  node: TreeNode;
  depth: number;
}

const hashModes: Exclude<HashMode, "off">[] = ["crc32", "md5", "sha1", "sha256"];
const hashLabels: Record<Exclude<HashMode, "off">, string> = {
  crc32: "CRC",
  md5: "MD5",
  sha1: "SHA1",
  sha256: "SHA256",
};
const imageExtensions = new Set(["png", "jpg", "jpeg", "gif", "webp", "bmp", "avif"]);
const audioExtensions = new Set(["ogg", "mp3", "wav", "flac", "m4a", "opus", "wma"]);
const videoExtensions = new Set(["mp4", "webm", "avi", "mkv", "wmv", "mpg", "mov"]);
const textExtensions = new Set(["txt", "md", "html", "htm", "pdf", "csv", "ini", "json", "xml"]);
const hexQuery = /^[0-9a-f]{8,}$/;

export function RomFileTree({
  workCode,
  romPosition,
  fileCount,
  gateway,
}: {
  workCode: string;
  romPosition: number;
  fileCount: number | null;
  gateway: CatalogDetailGateway;
}) {
  const { locale, t } = usePresentation();
  const [open, setOpen] = useState(false);
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const [search, setSearch] = useState("");
  const [hashMode, setHashMode] = useState<HashMode>("off");
  const searchInput = useRef<HTMLInputElement>(null);
  const result = useQuery({
    queryKey: ["catalog-rom-contents", workCode, romPosition],
    queryFn: () => gateway.readRomContents(workCode, romPosition),
    enabled: open,
    retry: false,
    staleTime: Number.POSITIVE_INFINITY,
  });
  const tree = useMemo(() => buildRomTree(result.data?.entries ?? []), [result.data]);
  const directoryIds = useMemo(() => collectDirectoryIds(tree), [tree]);
  const visibleNodes = useMemo(
    () => flattenTree(search ? filterTree(tree, search) : tree, expanded, Boolean(search)),
    [expanded, search, tree],
  );
  const displayCount = fileCount ?? 0;
  const allExpanded = directoryIds.length > 0 && directoryIds.every((id) => expanded.has(id));

  const toggleOpen = () => {
    if (displayCount === 0) return;
    setOpen((value) => !value);
    if (open) {
      setExpanded(new Set());
      setSearch("");
    }
  };

  return (
    <div className="rft">
      <button type="button" className="rft-toggle" onClick={toggleOpen} disabled={displayCount === 0}>
        <span className="rft-toggle-label">{open ? t("detail.files.hide") : t("detail.files.show")}</span>
        <span className="rft-toggle-count">({displayCount.toLocaleString(locale)})</span>
      </button>
      {open && (
        <div className="rft-panel">
          {result.isPending && <p className="rft-note">{t("detail.files.loadingContents")}</p>}
          {result.isError && <p className="rft-note rft-error">{t("detail.files.notIncluded")}</p>}
          {result.isSuccess && tree.length === 0 && <p className="rft-note">{t("detail.files.noneIndexed")}</p>}
          {result.isSuccess && tree.length > 0 && (
            <>
              <div className="rft-toolbar">
                <label className="rft-search">
                  <Search className="rft-icon-xs" aria-hidden="true" />
                  <input
                    ref={searchInput}
                    type="text"
                    className="rft-search-input"
                    placeholder={t("detail.files.searchPlaceholder")}
                    value={search}
                    onChange={(event) => setSearch(event.target.value)}
                  />
                  {search && (
                    <button
                      type="button"
                      className="rft-search-clear"
                      onClick={() => {
                        setSearch("");
                        searchInput.current?.focus();
                      }}
                      aria-label={t("detail.files.clearSearch")}
                    >
                      <X className="rft-icon-xs" />
                    </button>
                  )}
                </label>
                <div className="rft-seg" role="group" aria-label={t("detail.files.hashColumn")}>
                  <button
                    type="button"
                    className={`rft-seg-btn ${hashMode === "off" ? "is-active" : ""}`}
                    onClick={() => setHashMode("off")}
                    aria-label={t("detail.files.hideHashColumn")}
                    aria-pressed={hashMode === "off"}
                  >
                    <EyeOff className="rft-icon-xs" />
                  </button>
                  {hashModes.map((mode) => (
                    <button
                      type="button"
                      className={`rft-seg-btn ${hashMode === mode ? "is-active" : ""}`}
                      onClick={() => setHashMode(mode)}
                      aria-pressed={hashMode === mode}
                      key={mode}
                    >
                      {hashLabels[mode]}
                    </button>
                  ))}
                </div>
                {!search && directoryIds.length > 0 && (
                  <button
                    type="button"
                    className="rft-tool-btn"
                    onClick={() => setExpanded(allExpanded ? new Set() : new Set(directoryIds))}
                  >
                    {allExpanded ? t("detail.files.collapseAll") : t("detail.files.expandAll")}
                  </button>
                )}
              </div>
              <div className="rft-tree" role="tree" aria-label={t("detail.files.archiveContents")}>
                {visibleNodes.length > 0 ? visibleNodes.map(({ node, depth }) => (
                  <FileTreeRow
                    node={node}
                    depth={depth}
                    expanded={expanded.has(node.id)}
                    hashMode={hashMode}
                    locale={locale}
                    copyLabel={t("detail.files.copyInfo")}
                    fileLabel={t("detail.files.file")}
                    sizeLabel={t("detail.files.size")}
                    bytesLabel={t("detail.files.bytes")}
                    onToggle={() => setExpanded((current) => toggleSetValue(current, node.id))}
                    key={node.id}
                  />
                )) : <p className="rft-note">{t("detail.files.noSearchMatches")}</p>}
              </div>
              {result.data.truncated && <p className="rft-note">{t("detail.files.truncated")}</p>}
            </>
          )}
        </div>
      )}
    </div>
  );
}

function FileTreeRow({
  node,
  depth,
  expanded,
  hashMode,
  locale,
  copyLabel,
  fileLabel,
  sizeLabel,
  bytesLabel,
  onToggle,
}: {
  node: TreeNode;
  depth: number;
  expanded: boolean;
  hashMode: HashMode;
  locale: string;
  copyLabel: string;
  fileLabel: string;
  sizeLabel: string;
  bytesLabel: string;
  onToggle: () => void;
}) {
  const directory = node.children !== undefined;
  const entry = node.entry;
  const activeHash = entry && hashMode !== "off" ? entry[hashMode] : "";
  const copyValue = entry
    ? (hashMode === "off" ? formatEntryInfo(entry, locale, { file: fileLabel, size: sizeLabel, bytes: bytesLabel }) : activeHash)
    : "";
  return (
    <div
      className={`rft-row ${directory ? "rft-row-dir" : ""}`}
      style={{ paddingLeft: `${8 + depth * 18}px` }}
      onClick={directory ? onToggle : undefined}
      role="treeitem"
      aria-expanded={directory ? expanded : undefined}
      title={entry ? formatEntryInfo(entry, locale, { file: fileLabel, size: sizeLabel, bytes: bytesLabel }) : node.name}
    >
      <span className={`rft-chevron ${expanded ? "rft-chevron-open" : ""}`} aria-hidden="true">
        {directory && <ChevronRight className="rft-icon-xs" />}
      </span>
      {directory ? <Folder className="rft-icon rft-icon-folder" /> : <EntryIcon extension={entry?.extension} />}
      <span className="rft-name">{node.name}</span>
      {entry && (
        <span className="rft-meta">
          {entry.hashStatus === "failed" && <CircleAlert className="rft-icon-xs rft-failed" />}
          {hashMode !== "off" && (
            <span className={`rft-hash ${activeHash ? "" : "rft-hash-missing"}`}>
              {activeHash ? truncateHash(activeHash.toLowerCase()) : "—"}
            </span>
          )}
          <span className="rft-size">{humanSize(entry.size, locale, bytesLabel)}</span>
          {copyValue && <CopyButton value={copyValue} label={copyLabel} />}
        </span>
      )}
    </div>
  );
}

function EntryIcon({ extension }: { extension?: string }) {
  const normalized = (extension ?? "").toLowerCase();
  let Icon: ComponentType<SVGProps<SVGSVGElement>> = File;
  if (imageExtensions.has(normalized)) Icon = ImageIcon;
  else if (audioExtensions.has(normalized)) Icon = Music;
  else if (videoExtensions.has(normalized)) Icon = Film;
  else if (textExtensions.has(normalized)) Icon = FileText;
  return <Icon className="rft-icon rft-icon-file" aria-hidden="true" />;
}

function CopyButton({ value, label }: { value: string; label: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      className="rft-copy"
      aria-label={label}
      onClick={(event) => {
        event.stopPropagation();
        void navigator.clipboard.writeText(value).then(() => {
          setCopied(true);
          window.setTimeout(() => setCopied(false), 1500);
        });
      }}
    >
      {copied ? <Check className="rft-icon-xs" /> : <Copy className="rft-icon-xs" />}
    </button>
  );
}

export function buildRomTree(entries: CatalogRomEntry[]): TreeNode[] {
  interface Directory {
    name: string;
    path: string;
    directories: Map<string, Directory>;
    files: Array<{ name: string; entry: CatalogRomEntry }>;
  }
  const root: Directory = { name: "", path: "", directories: new Map(), files: [] };
  for (const entry of entries) {
    const parts = entry.path.split("/").filter(Boolean);
    if (parts.length === 0) continue;
    let directory = root;
    for (const part of parts.slice(0, -1)) {
      const path = `${directory.path}${part}/`;
      const child = directory.directories.get(part) ?? { name: part, path, directories: new Map(), files: [] };
      directory.directories.set(part, child);
      directory = child;
    }
    const name = parts.at(-1) ?? entry.path;
    if (entry.isDirectory) {
      const path = `${directory.path}${name}/`;
      if (!directory.directories.has(name)) directory.directories.set(name, { name, path, directories: new Map(), files: [] });
    } else {
      directory.files.push({ name, entry });
    }
  }
  const convert = (directory: Directory): TreeNode[] => [
    ...[...directory.directories.values()].sort((left, right) => left.name.localeCompare(right.name)).map((child) => ({
      id: `d:${child.path}`,
      name: child.name,
      children: convert(child),
    })),
    ...directory.files.sort((left, right) => left.name.localeCompare(right.name)).map(({ name, entry }) => ({
      id: `f:${entry.entryIndex}`,
      name,
      entry,
    })),
  ];
  return convert(root);
}

function filterTree(nodes: TreeNode[], rawSearch: string): TreeNode[] {
  const search = rawSearch.trim().toLowerCase();
  if (!search) return nodes;
  return nodes.flatMap((node) => {
    if (node.children) {
      const children = filterTree(node.children, search);
      return children.length > 0 || node.name.toLowerCase().includes(search) ? [{ ...node, children }] : [];
    }
    return node.entry && matchesEntry(node.entry, search) ? [node] : [];
  });
}

function matchesEntry(entry: CatalogRomEntry, search: string): boolean {
  if (entry.path.toLowerCase().includes(search)) return true;
  return hexQuery.test(search) && [entry.crc32, entry.md5, entry.sha1, entry.sha256].some((hash) => hash.toLowerCase().includes(search));
}

function flattenTree(nodes: TreeNode[], expanded: ReadonlySet<string>, forceOpen: boolean, depth = 0): VisibleNode[] {
  return nodes.flatMap((node) => [
    { node, depth },
    ...(node.children && (forceOpen || expanded.has(node.id)) ? flattenTree(node.children, expanded, forceOpen, depth + 1) : []),
  ]);
}

function collectDirectoryIds(nodes: TreeNode[]): string[] {
  return nodes.flatMap((node) => node.children ? [node.id, ...collectDirectoryIds(node.children)] : []);
}

function toggleSetValue(values: ReadonlySet<string>, value: string): ReadonlySet<string> {
  const next = new Set(values);
  if (next.has(value)) next.delete(value);
  else next.add(value);
  return next;
}

function truncateHash(hash: string): string {
  return hash.length <= 24 ? hash : `${hash.slice(0, 10)}***${hash.slice(-10)}`;
}

export function humanSize(raw: string | null, locale: string, bytesLabel: string): string {
  if (!raw) return "—";
  const bytes = Number(raw);
  if (!Number.isFinite(bytes)) return raw;
  const units = [bytesLabel, "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit += 1;
  }
  return unit === 0
    ? `${bytes.toLocaleString(locale)} ${bytesLabel}`
    : `${value.toLocaleString(locale, { maximumFractionDigits: 1 })} ${units[unit]}`;
}

export function formatEntryInfo(
  entry: CatalogRomEntry,
  locale: string,
  labels: { file: string; size: string; bytes: string },
): string {
  const values = [
    `${labels.file}: ${entry.path}`,
    entry.size
      ? `${labels.size}: ${humanSize(entry.size, locale, labels.bytes)} (${Number(entry.size).toLocaleString(locale)} ${labels.bytes})`
      : "",
    entry.crc32 ? `CRC32: ${entry.crc32}` : "",
    entry.md5 ? `MD5: ${entry.md5}` : "",
    entry.sha1 ? `SHA1: ${entry.sha1}` : "",
    entry.sha256 ? `SHA256: ${entry.sha256}` : "",
  ];
  return values.filter(Boolean).join("\n");
}
