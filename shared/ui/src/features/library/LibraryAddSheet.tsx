import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  Archive,
  Check,
  ChevronDown,
  FolderInput,
  FolderOpen,
  FolderSearch,
  FolderTree,
  LoaderCircle,
  Plus,
  X,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { useDocumentScrollLock } from "../../app/useDocumentScrollLock";
import { formatByteSize } from "../../formatByteSize";
import type { MessageKey } from "../../i18n/catalogs";
import { usePresentation } from "../../preferences/PresentationProvider";
import { CatalogWorkPicker } from "../catalog/CatalogWorkPicker";
import type { CatalogDetailGateway, CatalogWork } from "../catalog/types";
import { WorkCardImage } from "../catalog/WorkCardImage";
import { findingWorkCodes } from "../scanner/scanFindings";
import type { ScanInstallGateway } from "../scanner/types";
import type { SearchGateway } from "../search/types";
import {
  addable,
  addFindingKey,
  addFindingName,
  addItem,
  addStatus,
  folderRelease,
  jobFor,
  jobPercent,
  libraryAddSettled,
  type LibraryAddChoice,
  type LibraryAddFinding,
  type LibraryAddGateway,
  type LibraryAddJob,
  type LibraryAddSource,
  type LibraryAddStatus,
  type LibraryAddView,
} from "./libraryAdd";
import { OWN_WORK_DETAILS_KEY } from "./ownWorkDetails";

const SOURCES: ReadonlyArray<{ source: LibraryAddSource; title: MessageKey; help: MessageKey; icon: typeof FolderInput }> = [
  { source: "folders", title: "library.add.folders", help: "library.add.foldersHelp", icon: FolderInput },
  { source: "archives", title: "library.add.archives", help: "library.add.archivesHelp", icon: Archive },
  { source: "scan", title: "library.add.scan", help: "library.add.scanHelp", icon: FolderSearch },
];

const STATUS_LABELS: Record<LibraryAddStatus, MessageKey> = {
  catalog: "library.add.status.catalog",
  newer: "library.add.status.newer",
  choose: "library.add.status.choose",
  own: "library.add.status.own",
  library: "library.add.status.library",
  incomplete: "library.add.status.incomplete",
  mixed: "library.add.status.mixed",
};

/** Keeps the current "+ Add" in step with the app, which carries it on in the background. */
export function useLibraryAdd(gateway: LibraryAddGateway | undefined) {
  const [view, setView] = useState<LibraryAddView | null>(null);
  useEffect(() => {
    if (!gateway) return;
    let active = true;
    let stop: (() => void) | undefined;
    const accept = (next: LibraryAddView | null) => {
      if (!active) return;
      setView((current) => (
        !next || !current || next.batchId !== current.batchId || next.revision >= current.revision ? next : current
      ));
    };
    void gateway.readLibraryAdd().then(accept, () => undefined);
    void gateway.subscribeLibraryAdd(accept).then((unsubscribe) => {
      if (active) stop = unsubscribe;
      else unsubscribe();
    });
    return () => {
      active = false;
      stop?.();
    };
  }, [gateway]);
  return [view, setView] as const;
}

/** The Library's "+ Add": work folders, archives, or a folder to scan for works. */
export function LibraryAddButton({
  gateway,
  installs,
  view,
  onView,
  onOpen,
}: {
  gateway: LibraryAddGateway;
  installs?: ScanInstallGateway;
  view: LibraryAddView | null;
  onView: (view: LibraryAddView) => void;
  onOpen: () => void;
}) {
  const { t } = usePresentation();
  const [open, setOpen] = useState(false);
  const [error, setError] = useState("");
  const menuRef = useRef<HTMLDivElement>(null);
  const busy = view !== null && !libraryAddSettled(view);

  useEffect(() => {
    if (!open) return;
    const close = (event: PointerEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    window.addEventListener("pointerdown", close);
    window.addEventListener("keydown", escape);
    return () => {
      window.removeEventListener("pointerdown", close);
      window.removeEventListener("keydown", escape);
    };
  }, [open]);

  const start = async (source: LibraryAddSource) => {
    setOpen(false);
    setError("");
    try {
      // Works are unpacked and moved into the install folder, so it is asked for first.
      if (installs) {
        const current = await installs.readInstalls();
        if (!current.installFolder && !(await installs.selectInstallFolder())?.installFolder) return;
      }
      const started = await gateway.startLibraryAdd(source);
      if (started) {
        onView(started);
        onOpen();
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  };

  return (
    <div className="library-add" ref={menuRef}>
      <button
        className="library-add-button"
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => (busy ? onOpen() : setOpen((value) => !value))}
      >
        {busy ? <LoaderCircle className="library-spin" aria-hidden="true" /> : <Plus aria-hidden="true" />}
        {t(busy ? "library.add.busy" : "library.add.button")}
      </button>
      {open ? (
        <div className="library-add-menu" role="menu" aria-label={t("library.add.menu")}>
          {SOURCES.map(({ source, title, help, icon: Icon }) => (
            <button className="library-add-option" type="button" role="menuitem" key={source} onClick={() => void start(source)}>
              <span aria-hidden="true"><Icon /></span>
              <span>
                <strong>{t(title)}</strong>
                <small>{t(help)}</small>
              </span>
            </button>
          ))}
        </div>
      ) : null}
      {error ? (
        <div className="library-add-error" role="alert">
          <AlertTriangle aria-hidden="true" />{t("common.requestFailed", { error })}
          <button type="button" aria-label={t("notice.close")} onClick={() => setError("")}><X aria-hidden="true" /></button>
        </div>
      ) : null}
    </div>
  );
}

/** Reviews what "+ Add" found, then follows each work into the install folder. */
export function LibraryAddSheet({
  view,
  gateway,
  installs,
  catalogGateway,
  searchGateway,
  onHide,
  onManage,
}: {
  view: LibraryAddView;
  gateway: LibraryAddGateway;
  installs?: ScanInstallGateway;
  catalogGateway: Pick<CatalogDetailGateway, "readWorks">;
  searchGateway?: Pick<SearchGateway, "search">;
  onHide: () => void;
  onManage: (installationId: string) => void;
}) {
  const { locale, t } = usePresentation();
  const preferEnglish = locale !== "ja-JP";
  const queryClient = useQueryClient();
  const [unticked, setUnticked] = useState<ReadonlySet<string>>(new Set());
  const [choices, setChoices] = useState<Record<string, LibraryAddChoice>>({});
  const [choosing, setChoosing] = useState<string | null>(null);
  const [deleteArchives, setDeleteArchives] = useState(view.deleteArchives);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useDocumentScrollLock(true);

  const codes = useMemo(() => {
    const listed = findingWorkCodes(view.findings.map((item) => item.finding));
    const picked = Object.values(choices).flatMap((choice) => (choice.kind === "work" ? [choice.workCode] : []));
    return [...new Set([...listed, ...picked])].sort();
  }, [choices, view.findings]);
  const works = useQuery({
    queryKey: ["catalog-works", codes],
    queryFn: () => catalogGateway.readWorks(codes),
    enabled: codes.length > 0,
    staleTime: 5 * 60_000,
  });
  const workByCode = useMemo(() => new Map((works.data ?? []).map((work) => [work.code, work])), [works.data]);
  const installFolder = useQuery({
    queryKey: ["scanner", "installs"],
    queryFn: () => installs!.readInstalls(),
    enabled: Boolean(installs),
  });

  const rows = useMemo(() => view.findings.map((item) => {
    const key = addFindingKey(item);
    const name = addFindingName(item, view.sources);
    const work = item.finding.workCode ? workByCode.get(item.finding.workCode) : undefined;
    const status = addStatus(item.finding, name, work);
    return { item, key, name, work, status };
  }), [view.findings, view.sources, workByCode]);

  // Works arriving in the Library show up there while the rest are still on their way.
  const doneJobs = view.jobs.filter((job) => job.state === "done").length;
  useEffect(() => {
    if (!doneJobs) return;
    void queryClient.invalidateQueries({ queryKey: ["library"] });
    void queryClient.invalidateQueries({ queryKey: OWN_WORK_DETAILS_KEY });
  }, [doneJobs, queryClient]);

  const reviewing = view.phase === "review";
  const items = rows.flatMap((row) => {
    if (!reviewing || !addable(row.status) || unticked.has(row.key)) return [];
    const request = addItem(row.item, row.status, choices[row.key]);
    return request ? [{ request, row }] : [];
  });
  const waiting = rows.filter((row) => addable(row.status) && !unticked.has(row.key) && !addItem(row.item, row.status, choices[row.key]));
  const archivesTicked = items.some(({ row }) => row.item.placement === "unpack");

  const confirm = async () => {
    setBusy(true);
    setError("");
    try {
      await gateway.confirmLibraryAdd({ batchId: view.batchId, items: items.map(({ request }) => request), deleteArchives });
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };
  const cancel = async () => {
    setBusy(true);
    try {
      // A copy under way is rolled back first; the sheet closes once nothing is left running.
      const next = await gateway.cancelLibraryAdd(view.batchId);
      if (next && libraryAddSettled(next)) {
        await gateway.dismissLibraryAdd(view.batchId).catch(() => undefined);
        onHide();
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };
  const close = async () => {
    if (libraryAddSettled(view)) await gateway.dismissLibraryAdd(view.batchId).catch(() => undefined);
    onHide();
  };

  const toggle = (key: string) => setUnticked((current) => {
    const next = new Set(current);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    return next;
  });
  const choose = (key: string, choice: LibraryAddChoice) => {
    setChoices((current) => ({ ...current, [key]: choice }));
    setChoosing(null);
  };

  const heading = view.phase === "scanning"
    ? t("library.add.title.scanning")
    : view.phase === "review"
      ? rows.length === 1 ? t("library.add.title.foundOne") : t("library.add.title.found", { count: rows.length })
      : view.phase === "adding"
        ? t("library.add.title.adding")
        : view.phase === "cancelled" ? t("library.add.title.cancelled") : t("library.add.title.done");

  return (
    <div className="library-add-layer">
      <div className="library-add-backdrop" aria-hidden="true" onClick={() => void close()} />
      <section className="library-add-sheet" role="dialog" aria-modal="true" aria-labelledby="library-add-heading">
        <header className="library-add-head">
          <div>
            <span className="library-add-eyebrow">{t("library.add.eyebrow")}</span>
            <h2 id="library-add-heading">{heading}</h2>
            {view.phase === "review" ? <p>{t("library.add.reviewHelp")}</p> : null}
          </div>
          <button className="library-add-close" type="button" aria-label={t("library.add.hide")} onClick={() => void close()}>
            <X aria-hidden="true" />
          </button>
        </header>

        <div className="library-add-body">
          {view.phase === "scanning" ? (
            <ul className="library-add-sources">
              {view.sources.map((source) => (
                <li key={source.sourceId}>
                  {source.status === "failed"
                    ? <AlertTriangle aria-hidden="true" />
                    : source.status === "done" ? <Check aria-hidden="true" /> : <LoaderCircle className="library-spin" aria-hidden="true" />}
                  <strong>{source.displayName}</strong>
                  <span>
                    {source.status === "failed"
                      ? t("common.requestFailed", { error: source.error ?? "" })
                      : t(source.status === "done" ? "library.add.sourceDone" : source.status === "scanning" ? "library.add.sourceScanning" : "library.add.sourceQueued")}
                  </span>
                </li>
              ))}
            </ul>
          ) : rows.length === 0 ? (
            <p className="library-add-empty">{t("library.add.nothingFound")}</p>
          ) : (
            <ul className="library-add-rows">
              {rows.map((row) => (
                <AddRow
                  key={row.key}
                  row={row}
                  job={jobFor(view, row.item)}
                  reviewing={reviewing}
                  ticked={addable(row.status) && !unticked.has(row.key)}
                  choice={choices[row.key]}
                  choosing={choosing === row.key}
                  workByCode={workByCode}
                  preferEnglish={preferEnglish}
                  catalogGateway={catalogGateway}
                  searchGateway={searchGateway}
                  onToggle={() => toggle(row.key)}
                  onChoosing={() => setChoosing((current) => (current === row.key ? null : row.key))}
                  onChoose={(choice) => choose(row.key, choice)}
                  onManage={onManage}
                />
              ))}
            </ul>
          )}
          {view.sources.some((source) => source.status === "failed") && view.phase !== "scanning" ? (
            <div className="library-callout library-callout-error" role="alert">
              <AlertTriangle aria-hidden="true" />
              {view.sources.filter((source) => source.status === "failed").map((source) => `${source.displayName}: ${source.error ?? ""}`).join(" · ")}
            </div>
          ) : null}
          {error ? (
            <div className="library-callout library-callout-error" role="alert">
              <AlertTriangle aria-hidden="true" />{t("common.requestFailed", { error })}
            </div>
          ) : null}
        </div>

        <footer className="library-add-foot">
          {reviewing ? (
            <>
              <div className="library-add-destination">
                <FolderTree aria-hidden="true" />
                <span>
                  <strong>{t("library.add.destinationTitle")}</strong>
                  <small>{t("library.add.destinationHelp")}</small>
                  {installFolder.data?.installFolder ?? view.installFolder ? (
                    <code>{installFolder.data?.installFolder ?? view.installFolder}</code>
                  ) : null}
                </span>
                {installs ? (
                  <button
                    className="library-manage-button"
                    type="button"
                    onClick={() => void installs.selectInstallFolder().then(() => installFolder.refetch())}
                  >
                    {t("library.add.changeFolder")}
                  </button>
                ) : null}
              </div>
              {archivesTicked ? (
                <label className="library-add-check">
                  <input type="checkbox" checked={deleteArchives} onChange={(event) => setDeleteArchives(event.currentTarget.checked)} />
                  {t("library.add.deleteArchives")}
                </label>
              ) : null}
              <div className="library-add-actions">
                {waiting.length ? <span className="library-add-waiting">{t("library.add.pickFirst", { count: waiting.length })}</span> : null}
                <button className="library-manage-button" type="button" disabled={busy} onClick={() => void cancel()}>
                  {t("common.cancel")}
                </button>
                <button className="library-manage-button is-primary" type="button" disabled={busy || items.length === 0} onClick={() => void confirm()}>
                  {busy ? <LoaderCircle className="library-spin" aria-hidden="true" /> : null}
                  {items.length === 1 ? t("library.add.addOne") : t("library.add.addMany", { count: items.length })}
                </button>
              </div>
            </>
          ) : (
            <div className="library-add-actions">
              {view.phase === "scanning" || view.phase === "adding" ? (
                <button className="library-manage-button" type="button" disabled={busy} onClick={() => void cancel()}>
                  {t(view.phase === "scanning" ? "common.cancel" : "library.add.cancelRest")}
                </button>
              ) : null}
              <button className="library-manage-button is-primary" type="button" onClick={() => void close()}>
                {t(libraryAddSettled(view) ? "library.add.close" : "library.add.hide")}
              </button>
            </div>
          )}
        </footer>
      </section>
    </div>
  );
}

interface AddRowModel {
  item: LibraryAddFinding;
  key: string;
  name: string;
  work: CatalogWork | undefined;
  status: LibraryAddStatus;
}

function AddRow({
  row,
  job,
  reviewing,
  ticked,
  choice,
  choosing,
  workByCode,
  preferEnglish,
  catalogGateway,
  searchGateway,
  onToggle,
  onChoosing,
  onChoose,
  onManage,
}: {
  row: AddRowModel;
  job: LibraryAddJob | undefined;
  reviewing: boolean;
  ticked: boolean;
  choice: LibraryAddChoice | undefined;
  choosing: boolean;
  workByCode: ReadonlyMap<string, CatalogWork>;
  preferEnglish: boolean;
  catalogGateway: Pick<CatalogDetailGateway, "readWorks">;
  searchGateway?: Pick<SearchGateway, "search">;
  onToggle: () => void;
  onChoosing: () => void;
  onChoose: (choice: LibraryAddChoice) => void;
  onManage: (installationId: string) => void;
}) {
  const { locale, t } = usePresentation();
  const { item, name, status } = row;
  const { finding } = item;
  const chosenWork = choice?.kind === "work" ? workByCode.get(choice.workCode) : undefined;
  const work = chosenWork ?? (choice?.kind === "own" ? undefined : row.work);
  const own = choice?.kind === "own" || (!choice && status === "own");
  const title = work ? workTitle(work, preferEnglish) : name;
  const circle = work?.circles[0];
  const code = choice?.kind === "work" ? choice.workCode : choice?.kind === "own" ? null : finding.workCode;
  const release = folderRelease(name);
  const addableRow = addable(status);
  const note = rowNote(row, choice, t);

  return (
    <li className={`library-add-row is-${status}${!addableRow ? " is-blocked" : ""}`}>
      {reviewing ? (
        <input
          type="checkbox"
          aria-label={t("library.add.tick", { name: title })}
          checked={ticked}
          disabled={!addableRow}
          onChange={onToggle}
        />
      ) : <span aria-hidden="true" />}
      <span className="library-add-cover" aria-hidden="true">
        {work ? (
          <WorkCardImage code={work.code} title={title} mainImageUrls={work.mainImageUrls} thumbnailUrls={work.thumbnailUrls} />
        ) : finding.kind === "archive" ? <Archive /> : <FolderOpen />}
      </span>
      <div className="library-add-main">
        <strong title={title}>{title}</strong>
        <span className="library-add-meta">
          {circle ? <span>{preferEnglish && circle.nameEnglish.trim() ? circle.nameEnglish : circle.name}</span> : null}
          {code ? <code>{code}</code> : null}
          {release ? <span>{release.version ? t("library.add.release", { date: release.date, version: release.version }) : release.date}</span> : null}
          <span>
            {finding.fileCount === 1 ? t("library.home.filesOne") : t("library.home.files", { count: finding.fileCount.toLocaleString(locale) })}
            {" · "}{formatByteSize(finding.sizeBytes, locale)}
          </span>
          {addableRow ? <span>{t(placementKey(item.placement))}</span> : null}
        </span>
        {note ? <span className={`library-add-note is-${status}`}>{note}</span> : null}
        {reviewing && status === "choose" && choosing ? (
          <div className="library-add-choose">
            <div className="library-add-candidates" role="group" aria-label={t("library.add.candidates")}>
              {finding.candidates.map((candidate) => {
                const candidateWork = workByCode.get(candidate.workCode);
                return (
                  <button className="library-manage-button" type="button" key={candidate.workCode} onClick={() => onChoose({ kind: "work", workCode: candidate.workCode })}>
                    <code>{candidate.workCode}</code>{candidateWork ? ` · ${workTitle(candidateWork, preferEnglish)}` : null}
                  </button>
                );
              })}
            </div>
            <CatalogWorkPicker
              catalogGateway={catalogGateway}
              searchGateway={searchGateway}
              onPick={(picked) => onChoose({ kind: "work", workCode: picked.code })}
            />
            <button className="library-manage-link" type="button" onClick={() => onChoose({ kind: "own" })}>
              {t("library.add.addAsOwn")}
            </button>
          </div>
        ) : null}
      </div>
      <div className="library-add-side">
        {job ? <JobState job={job} own={own} onManage={onManage} /> : (
          <>
            <span className={`library-add-status is-${choice ? (choice.kind === "own" ? "own" : "catalog") : status}`}>
              {t(choice ? (choice.kind === "own" ? "library.add.status.own" : "library.add.status.picked") : STATUS_LABELS[status])}
            </span>
            {reviewing && status === "choose" ? (
              <button className="library-manage-button" type="button" aria-expanded={choosing} onClick={onChoosing}>
                {t(choice ? "library.add.change" : "library.add.pick")}<ChevronDown aria-hidden="true" />
              </button>
            ) : null}
          </>
        )}
      </div>
    </li>
  );
}

function JobState({ job, own, onManage }: { job: LibraryAddJob; own: boolean; onManage: (installationId: string) => void }) {
  const { t } = usePresentation();
  const percent = jobPercent(job);
  if (job.state === "done") {
    return (
      <span className="library-add-job is-done">
        <span><Check aria-hidden="true" />{t("library.add.job.done")}</span>
        {job.warning ? <small>{job.warning}</small> : null}
        {own && job.installationId ? (
          <button className="library-manage-button" type="button" onClick={() => onManage(job.installationId!)}>
            {t("library.add.addMetadata")}
          </button>
        ) : null}
      </span>
    );
  }
  if (job.state === "failed") {
    return <span className="library-add-job is-failed" role="alert"><AlertTriangle aria-hidden="true" />{job.error ?? t("library.add.job.failed")}</span>;
  }
  if (job.state === "cancelled") return <span className="library-add-job">{t("library.add.job.cancelled")}</span>;
  if (job.state === "queued") return <span className="library-add-job">{t("library.add.job.queued")}</span>;
  return (
    <span className="library-add-job is-running">
      <span><LoaderCircle className="library-spin" aria-hidden="true" />{t(runningKey(job))}{percent !== null ? ` ${percent}%` : ""}</span>
      {percent !== null ? <span className="library-add-progress" aria-hidden="true"><i style={{ width: `${percent}%` }} /></span> : null}
    </span>
  );
}

function rowNote(
  row: AddRowModel,
  choice: LibraryAddChoice | undefined,
  t: (key: MessageKey, values?: Record<string, string | number>) => string,
): string {
  const { finding } = row.item;
  if (choice) return "";
  switch (row.status) {
    case "newer": return t("library.add.note.newer", { date: row.work?.latestUpdate?.date ?? "" });
    case "choose": return t("library.add.note.choose");
    case "own": return t("library.add.note.own");
    case "library": return t("library.add.note.library");
    case "mixed": return t("library.add.note.mixed");
    case "incomplete": {
      const missing = finding.incompleteSet?.missingParts ?? [];
      const renames = finding.incompleteSet?.renames ?? [];
      return [
        missing.length ? t("library.add.note.missing", { parts: missing.join(", ") }) : "",
        ...renames.map((rename) => t("scanner.incompleteRename", { from: rename.from, to: rename.to })),
      ].filter(Boolean).join(" ") || t("library.add.note.incomplete");
    }
    default: return "";
  }
}

function placementKey(placement: LibraryAddFinding["placement"]): MessageKey {
  switch (placement) {
    case "move": return "library.add.placement.move";
    case "copy": return "library.add.placement.copy";
    case "in_place": return "library.add.placement.inPlace";
    case "unpack": return "library.add.placement.unpack";
  }
}

function runningKey(job: LibraryAddJob): MessageKey {
  switch (job.kind) {
    case "move": return "library.add.job.moving";
    case "copy": return "library.add.job.copying";
    case "unpack": return "library.add.job.unpacking";
    case "add": return "library.add.job.adding";
  }
}

function workTitle(work: CatalogWork, preferEnglish: boolean): string {
  return preferEnglish && work.titleEnglish.trim() ? work.titleEnglish : work.title;
}
