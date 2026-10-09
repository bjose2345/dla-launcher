import { useMemo, useState } from "react";
import {
  createSortedRowModel,
  createColumnHelper,
  flexRender,
  rowSortingFeature,
  tableFeatures,
  useTable,
  type SortingState,
} from "@tanstack/react-table";
import { ChevronDown, ChevronUp, ChevronsUpDown, ListChecks, Play } from "lucide-react";

import { usePresentation } from "../../preferences/PresentationProvider";
import { displayPath } from "../../displayPath";
import { installationFolder } from "./libraryPresentation";
import { LibraryArtwork } from "./LibraryArtwork";
import { collectionState, type LibraryCollectionEntry } from "./LibraryCollection";
import type { ManageTab } from "./LibraryManageDrawer";
import { LibraryWorkMenu } from "./LibraryWorkMenu";
import {
  libraryContentKind,
  libraryContentKindLabel,
  libraryDisplayCreator,
  libraryDisplayTitle,
  type LibraryContentKind,
} from "./libraryHome";
import { mediaActionMessageKey } from "./mediaSession";

interface LibraryRow {
  entry: LibraryCollectionEntry;
  title: string;
  creator: string;
  kind: LibraryContentKind;
  files: number;
  progress: number | null;
}

const features = tableFeatures({
  rowSortingFeature,
  sortedRowModel: createSortedRowModel(),
});
const columnHelper = createColumnHelper<typeof features, LibraryRow>();

export function LibraryTable({
  entries,
  onActivate,
  onManage,
  onOpenWork,
  onBrowse,
}: {
  entries: LibraryCollectionEntry[];
  onActivate: (entry: LibraryCollectionEntry) => void;
  onManage: (installationId: string, tab: ManageTab) => void | Promise<void>;
  onOpenWork: (code: string) => void | Promise<void>;
  onBrowse?: (installationId: string) => void;
}) {
  const { locale, t } = usePresentation();
  const [sorting, setSorting] = useState<SortingState>([{ id: "title", desc: false }]);
  const preferEnglish = locale !== "ja-JP";

  const rows = useMemo<LibraryRow[]>(() => entries.map((entry) => {
    const resume = entry.resume;
    return {
      entry,
      title: libraryDisplayTitle(entry.installation, entry.work, preferEnglish),
      creator: libraryDisplayCreator(entry.installation, entry.work, preferEnglish),
      kind: libraryContentKind(entry.installation, entry.action),
      files: entry.fileCount,
      progress: resume && resume.durationMs !== null && resume.durationMs > 0
        ? Math.round(Math.max(0, Math.min(100, (resume.positionMs / resume.durationMs) * 100)))
        : null,
    };
  }), [entries, preferEnglish]);

  const columns = useMemo(() => columnHelper.columns([
    columnHelper.accessor("title", {
      header: () => t("library.table.work"),
      cell: (context) => (
        <div className="library-table-work">
          <span className="library-table-thumb" data-library-kind={context.row.original.kind}>
            <LibraryArtwork
              kind={context.row.original.kind}
              title={context.getValue()}
              work={context.row.original.entry.work}
            />
          </span>
          <span className="library-table-name">
            <strong title={context.getValue()}>{context.getValue()}</strong>
            <small><code className="app-path">{displayPath(installationFolder(context.row.original.entry.installation))}</code></small>
          </span>
        </div>
      ),
    }),
    columnHelper.accessor("creator", {
      header: () => t("library.table.creator"),
      cell: (context) => <span title={context.getValue()}>{context.getValue()}</span>,
    }),
    columnHelper.accessor("kind", {
      header: () => t("library.table.kind"),
      cell: (context) => (
        <span className="library-table-kind" data-library-kind={context.getValue()}>
          {libraryContentKindLabel(context.getValue(), t)}
        </span>
      ),
    }),
    columnHelper.accessor("files", {
      header: () => t("library.table.files"),
      cell: (context) => <span className="library-table-number">{context.getValue()}</span>,
    }),
    columnHelper.accessor("progress", {
      header: () => t("library.table.progress"),
      sortUndefined: "last",
      cell: (context) => {
        const value = context.getValue();
        const state = collectionState(context.row.original.entry);
        if (value === null) {
          return (
            <span className="library-table-number">
              {state === "new" ? t("library.state.new") : "—"}
            </span>
          );
        }
        return (
          <span
            className="library-table-progress"
            aria-label={t("library.shelf.progress", { percent: value })}
          >
            <span className="library-table-bar"><i style={{ width: `${value}%` }} /></span>
            <span className="library-table-number">{value}%</span>
          </span>
        );
      },
    }),
    columnHelper.display({
      id: "actions",
      header: () => "",
      cell: (context) => {
        const { entry, title } = context.row.original;
        const { action, work } = entry;
        const activateLabel = action && action !== "launch_executable"
          ? t(mediaActionMessageKey(action))
          : action === "launch_executable"
            ? t("detail.play")
            : t("library.setUp");
        return (
          <span className="library-table-actions">
            <button
              className="library-table-open"
              type="button"
              aria-label={activateLabel}
              title={activateLabel}
              disabled={collectionState(entry) === "running"}
              onClick={() => onActivate(entry)}
            >
              {action ? <Play fill="currentColor" aria-hidden="true" /> : <ListChecks aria-hidden="true" />}
            </button>
            <LibraryWorkMenu
              className="library-table-menu"
              title={title}
              onOpenWork={work ? () => void onOpenWork(work.code) : undefined}
              onManage={(tab) => void onManage(entry.installation.id, tab)}
              onBrowse={onBrowse ? () => onBrowse(entry.installation.id) : undefined}
            />
          </span>
        );
      },
    }),
  ]), [t, onActivate, onManage, onOpenWork, onBrowse]);

  const table = useTable({
    features,
    data: rows,
    columns,
    state: { sorting },
    onSortingChange: setSorting,
  });

  if (!entries.length) {
    return <p className="library-collection-empty">{t("library.home.filterEmpty")}</p>;
  }

  return (
    <section className="library-table-section" aria-labelledby="library-table-title">
      <header className="library-section-heading">
        <div>
          <span>{t("library.home.collectionEyebrow")}</span>
          <h2 id="library-table-title">{t("library.home.collection")}</h2>
          <p>{t("library.table.help")}</p>
        </div>
      </header>
      <div className="library-table-scroll">
        <table className="library-table">
          <thead>
            {table.getHeaderGroups().map((headerGroup) => (
              <tr key={headerGroup.id}>
                {headerGroup.headers.map((header) => (
                  <th key={header.id} aria-sort={ariaSort(header.column.getIsSorted())}>
                    {header.column.getCanSort() ? (
                      <button type="button" onClick={header.column.getToggleSortingHandler()}>
                        {flexRender(header.column.columnDef.header, header.getContext())}
                        <SortIcon direction={header.column.getIsSorted()} />
                      </button>
                    ) : (
                      flexRender(header.column.columnDef.header, header.getContext())
                    )}
                  </th>
                ))}
              </tr>
            ))}
          </thead>
          <tbody>
            {table.getRowModel().rows.map((row) => (
              <tr key={row.id}>
                {row.getAllCells().map((cell) => (
                  <td key={cell.id}>{flexRender(cell.column.columnDef.cell, cell.getContext())}</td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}

function SortIcon({ direction }: { direction: false | "asc" | "desc" }) {
  if (direction === "asc") return <ChevronUp aria-hidden="true" />;
  if (direction === "desc") return <ChevronDown aria-hidden="true" />;
  return <ChevronsUpDown aria-hidden="true" />;
}

function ariaSort(direction: false | "asc" | "desc"): "ascending" | "descending" | "none" {
  if (direction === "asc") return "ascending";
  if (direction === "desc") return "descending";
  return "none";
}

