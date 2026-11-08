import {
  AppWindow,
  ChevronRight,
  EyeOff,
  File,
  FileArchive,
  FileText,
  Film,
  Folder,
  Image,
  Music,
  PenLine,
  Search,
  Smartphone,
  X,
} from "lucide-react";
import { createContext, useCallback, useContext, useMemo, useRef, useState } from "react";
import { Tree, type NodeApi, type NodeRendererProps, type TreeApi } from "react-arborist";

import { formatByteSize } from "../../formatByteSize";
import { usePresentation } from "../../preferences/PresentationProvider";
import type { MediaType } from "./types";

export interface InstalledFile {
  relativePath: string;
  mediaType: MediaType;
  sizeBytes: number | null;
  changed: boolean;
  hidden: boolean;
}

interface TreeNode {
  id: string;
  name: string;
  children?: TreeNode[];
  file?: InstalledFile;
}

// The picked file, for the rows: the tree takes its row as a component, so
// it has to stay the same component between renders or every row is redrawn.
const SelectedFile = createContext<string | null>(null);

const ROW_HEIGHT = 28;
const MAX_TREE_HEIGHT = 320;
const TREE_PADDING = 8;

/**
 * A work's installed files by folder, ported from the website's archive
 * browser: folders start closed, a search opens the ones with matches, and
 * clicking a file picks it for changing.
 */
export function InstalledFileTree({
  files,
  selected,
  onSelect,
}: {
  files: InstalledFile[];
  selected: string | null;
  onSelect: (relativePath: string) => void;
}) {
  const { t } = usePresentation();
  // Mirror of the open folders: the tree's height follows it, as measuring
  // the tree after a toggle runs before React has drawn it.
  const [openIds, setOpenIds] = useState<ReadonlySet<string>>(new Set());
  const [search, setSearch] = useState("");
  const treeRef = useRef<TreeApi<TreeNode> | null>(null);
  const searchInput = useRef<HTMLInputElement | null>(null);
  const data = useMemo(() => buildTree(files), [files]);

  // While searching, the tree shows the matching files and the folders
  // around them; ids as buildTree makes them.
  const searchIds = useMemo(() => {
    const term = search.trim().toLowerCase();
    if (!term) return null;
    const ids = new Set<string>();
    for (const file of files) {
      if (!file.relativePath.toLowerCase().includes(term)) continue;
      const parts = file.relativePath.split("/").filter(Boolean);
      let prefix = "";
      for (const part of parts.slice(0, -1)) {
        prefix += `${part}/`;
        ids.add(`d:${prefix}`);
      }
      ids.add(`f:${file.relativePath}`);
    }
    return ids;
  }, [files, search]);

  const visibleRows = useMemo(() => {
    let count = 0;
    const walk = (nodes: TreeNode[]) => {
      for (const node of nodes) {
        count += 1;
        if (node.children && openIds.has(node.id)) walk(node.children);
      }
    };
    walk(data);
    return count;
  }, [data, openIds]);

  const folderIds = useMemo(() => {
    const ids: string[] = [];
    const walk = (nodes: TreeNode[]) => {
      for (const node of nodes) {
        if (!node.children) continue;
        ids.push(node.id);
        walk(node.children);
      }
    };
    walk(data);
    return ids;
  }, [data]);

  const searching = searchIds !== null;
  const rowCount = searchIds ? searchIds.size : visibleRows;
  const height = Math.min(MAX_TREE_HEIGHT, Math.max(ROW_HEIGHT, rowCount * ROW_HEIGHT) + TREE_PADDING);
  const allOpen = folderIds.length > 0 && folderIds.every((id) => openIds.has(id));

  const changeSearch = useCallback((value: string) => {
    setSearch(value);
    // Leaving a search shows the closed folders again, in a fresh tree.
    if (value.trim() === "") setOpenIds(new Set());
  }, []);

  const toggled = useCallback((id: string) => {
    setOpenIds((previous) => {
      const next = new Set(previous);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const activate = useCallback((node: NodeApi<TreeNode>) => {
    if (node.data.file) onSelect(node.data.file.relativePath);
    else node.toggle();
  }, [onSelect]);

  const searchMatch = useCallback(
    (node: NodeApi<TreeNode>, term: string) => (
      node.data.file !== undefined && node.data.file.relativePath.toLowerCase().includes(term.toLowerCase())
    ),
    [],
  );

  return (
    <div className="library-file-tree">
      <div className="library-file-tree-tools">
        <label className="library-file-tree-search">
          <Search aria-hidden="true" />
          <input
            ref={searchInput}
            type="search"
            placeholder={t("library.files.find")}
            aria-label={t("library.files.find")}
            value={search}
            onChange={(event) => changeSearch(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape" && search) {
                event.stopPropagation();
                changeSearch("");
              }
            }}
          />
          {search ? (
            <button
              type="button"
              aria-label={t("library.files.clear")}
              title={t("library.files.clear")}
              onClick={() => {
                changeSearch("");
                searchInput.current?.focus();
              }}
            >
              <X aria-hidden="true" />
            </button>
          ) : null}
        </label>
        {!searching && folderIds.length > 0 ? (
          <button
            className="app-button"
            type="button"
            onClick={() => {
              if (allOpen) {
                treeRef.current?.closeAll();
                setOpenIds(new Set());
              } else {
                treeRef.current?.openAll();
                setOpenIds(new Set(folderIds));
              }
            }}
          >
            {t(allOpen ? "library.files.closeAll" : "library.files.openAll")}
          </button>
        ) : null}
      </div>
      {!data.length ? (
        <p className="library-manage-help">{t("library.noContent")}</p>
      ) : searchIds && searchIds.size === 0 ? (
        <p className="library-manage-help">{t("library.files.noMatches")}</p>
      ) : (
        <SelectedFile.Provider value={selected}>
          <div className="library-file-tree-box">
            <Tree<TreeNode>
              // A fresh tree on entering and leaving a search, so its open
              // folders can't drift from the height mirror.
              key={searching ? "search" : "browse"}
              ref={treeRef}
              data={data}
              width="100%"
              height={height}
              rowHeight={ROW_HEIGHT}
              indent={18}
              openByDefault={false}
              searchTerm={search.trim()}
              searchMatch={searchMatch}
              disableDrag
              disableDrop
              disableEdit
              disableMultiSelection
              onToggle={toggled}
              onActivate={activate}
              aria-label={t("library.manage.tab.installedFiles")}
            >
              {Row}
            </Tree>
          </div>
        </SelectedFile.Provider>
      )}
    </div>
  );
}

function Row({ node, style }: NodeRendererProps<TreeNode>) {
  const { locale, t } = usePresentation();
  const selected = useContext(SelectedFile);
  const file = node.data.file;
  const classes = ["library-file-row"];
  if (!file) classes.push("is-folder");
  if (file && file.relativePath === selected) classes.push("is-selected");
  if (file?.hidden) classes.push("is-hidden");
  return (
    <div style={style} className={classes.join(" ")} title={file?.relativePath}>
      <span className={`library-file-chevron${node.isOpen ? " is-open" : ""}`} aria-hidden="true">
        {file ? null : <ChevronRight />}
      </span>
      {file ? <FileIcon mediaType={file.mediaType} /> : <Folder className="library-file-folder" aria-hidden="true" />}
      <span className="library-file-name">{node.data.name}</span>
      {file ? (
        <span className="library-file-meta">
          {file.changed ? <PenLine className="is-changed" aria-label={t("library.manage.changedTitle")} /> : null}
          {file.hidden ? <EyeOff aria-label={t("library.files.hidden")} /> : null}
          {file.sizeBytes !== null ? <span>{formatByteSize(file.sizeBytes, locale)}</span> : null}
        </span>
      ) : null}
    </div>
  );
}

function FileIcon({ mediaType }: { mediaType: MediaType }) {
  const Icon = {
    executable: AppWindow,
    image: Image,
    audio: Music,
    video: Film,
    pdf: FileText,
    archive: FileArchive,
    android_package: Smartphone,
    directory: Folder,
    unknown: File,
  }[mediaType];
  return <Icon className="library-file-icon" aria-hidden="true" />;
}

/** Folders first, then files, each by name, as a file manager lists them. */
function buildTree(files: InstalledFile[]): TreeNode[] {
  interface Directory {
    name: string;
    directories: Map<string, Directory>;
    files: Array<{ name: string; file: InstalledFile }>;
  }
  const root: Directory = { name: "", directories: new Map(), files: [] };
  for (const file of files) {
    const parts = file.relativePath.split("/").filter(Boolean);
    const name = parts.pop();
    if (!name) continue;
    let directory = root;
    for (const part of parts) {
      let child = directory.directories.get(part);
      if (!child) {
        child = { name: part, directories: new Map(), files: [] };
        directory.directories.set(part, child);
      }
      directory = child;
    }
    directory.files.push({ name, file });
  }
  const nodes = (directory: Directory, prefix: string): TreeNode[] => [
    ...[...directory.directories.values()]
      .sort((left, right) => left.name.localeCompare(right.name))
      .map((child) => ({
        id: `d:${prefix}${child.name}/`,
        name: child.name,
        children: nodes(child, `${prefix}${child.name}/`),
      })),
    ...[...directory.files]
      .sort((left, right) => left.name.localeCompare(right.name))
      .map(({ name, file }) => ({ id: `f:${file.relativePath}`, name, file })),
  ];
  return nodes(root, "");
}
