import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  ArrowRight,
  Check,
  CheckCircle2,
  ChevronRight,
  Database,
  FileArchive,
  HardDrive,
  LoaderCircle,
  PackageOpen,
  ShieldAlert,
  ShieldCheck,
  Trash2,
  X,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";

import { formatByteSize } from "../../formatByteSize";
import {
  catalogProfileMessageKey,
  generationKindMessageKey,
} from "../../i18n/domainLabels";
import { usePresentation } from "../../preferences/PresentationProvider";
import { formatDuration } from "../library/LaunchHistory";
import {
  catalogImportIsTerminal,
  catalogImportPercent,
  catalogImportPhase,
  catalogImportPhases,
  type CatalogGenerationSummary,
  type CatalogImportGateway,
  type CatalogImportPhase,
  type CatalogImportPreview,
  type CatalogImportProgress,
} from "./types";

const progressKey = ["catalog-import", "progress"] as const;
const generationKey = ["catalog-import", "generations"] as const;

const phaseLabels: Record<CatalogImportPhase, "import.phase.checking" | "import.phase.building" | "import.phase.details" | "import.phase.finishing"> = {
  checking: "import.phase.checking",
  building: "import.phase.building",
  details: "import.phase.details",
  finishing: "import.phase.finishing",
};

type ImportStep = "choose" | "review" | "run";

export function CatalogImportPage({
  gateway,
  onOpenCatalog,
}: {
  gateway: CatalogImportGateway;
  onOpenCatalog?: () => void;
}) {
  const { locale, t } = usePresentation();
  const queryClient = useQueryClient();
  const [preview, setPreview] = useState<CatalogImportPreview | null>(null);
  const [removalTarget, setRemovalTarget] = useState<CatalogGenerationSummary | null>(null);
  const operationRef = useRef<HTMLDivElement>(null);
  const revealedOperationRef = useRef<string | null>(null);
  const progress = useQuery({
    queryKey: progressKey,
    queryFn: () => gateway.readProgress(),
    refetchInterval: (query) => {
      const current = query.state.data;
      return current && !catalogImportIsTerminal(current.stage) ? 700 : false;
    },
  });
  const generations = useQuery({
    queryKey: generationKey,
    queryFn: () => gateway.listGenerations(),
  });
  const choose = useMutation({
    mutationFn: async () => {
      const selected = await gateway.selectPackage();
      if (!selected) return null;
      return gateway.inspect(selected.accessHandle);
    },
    onSuccess: (value) => {
      if (value) setPreview(value);
    },
  });
  const start = useMutation({
    mutationFn: (accessHandle: string) => gateway.start(accessHandle),
    onSuccess: (value) => queryClient.setQueryData(progressKey, value),
  });
  const cancel = useMutation({
    mutationFn: (operationId: string) => gateway.cancel(operationId),
  });
  const activate = useMutation({
    mutationFn: (generationId: string) => gateway.activate(generationId),
    onSuccess: (value) => queryClient.setQueryData(progressKey, value),
  });
  const remove = useMutation({
    mutationFn: (generationId: string) => gateway.removeGeneration(generationId),
    onSuccess: () => setRemovalTarget(null),
    onSettled: () => queryClient.invalidateQueries({ queryKey: generationKey }),
  });

  useEffect(() => {
    let unsubscribe: (() => void) | undefined;
    let disposed = false;
    void gateway.subscribeProgress((value) => {
      queryClient.setQueryData(progressKey, value);
      if (catalogImportIsTerminal(value.stage)) {
        void refreshAfterCatalogOperation(queryClient);
      }
    }).then((listener) => {
      if (disposed) listener();
      else unsubscribe = listener;
    });
    return () => {
      disposed = true;
      unsubscribe?.();
    };
  }, [gateway, queryClient]);

  const current = progress.data;
  const active = Boolean(current && !catalogImportIsTerminal(current.stage));
  const finished = current?.stage === "completed";
  const stopped = current?.stage === "cancelled" || current?.stage === "failed";
  const pageError = choose.error ?? progress.error ?? generations.error;
  const step: ImportStep = active || finished || stopped ? "run" : preview ? "review" : "choose";
  const elapsedSeconds = useImportElapsed(current);

  useEffect(() => {
    if (
      !current
      || catalogImportIsTerminal(current.stage)
      || revealedOperationRef.current === current.operationId
    ) return;
    revealedOperationRef.current = current.operationId;
    operationRef.current?.scrollIntoView?.({ behavior: "smooth", block: "nearest" });
  }, [current]);

  const restart = () => {
    setPreview(null);
    start.reset();
    activate.reset();
    queryClient.setQueryData(progressKey, null);
  };
  const dismissOperation = () => {
    start.reset();
    activate.reset();
    queryClient.setQueryData(progressKey, null);
  };
  const browse = () => {
    start.reset();
    activate.reset();
    if (current && catalogImportIsTerminal(current.stage)) {
      queryClient.setQueryData(progressKey, null);
    }
    choose.mutate();
  };
  const catalogGenerations = generations.data ?? [];
  const installed = catalogGenerations.filter(
    (generation) => generation.kind !== "embedded" || generation.workCount > 0,
  );
  const activeGeneration = catalogGenerations.find((generation) => generation.state === "active") ?? null;

  return (
    <main className="import-page">
      <section className="import-masthead">
        <div className="import-masthead-title">
          <span className="import-eyebrow"><FileArchive aria-hidden="true" />{t("import.eyebrow")}</span>
          <div className="import-masthead-heading">
            <h1>{t("import.title")}</h1>
            {installed.length ? (
              <span className="import-count">{t("import.installedCount", { count: installed.length })}</span>
            ) : null}
          </div>
          <p>{t("import.description")}</p>
        </div>
        {step === "run" && current?.operationKind === "activation" ? (
          <CatalogActivationStatus stage={current.stage} />
        ) : (
          <ImportStepper step={step} complete={Boolean(finished)} />
        )}
      </section>

      <div className="import-body">
        {pageError ? (
          <ImportCallout
            tone="error"
            icon={<ShieldAlert />}
            text={t("common.requestFailed", { error: String(pageError) })}
          />
        ) : null}

        {step === "choose" ? <ChooseStep busy={choose.isPending} onBrowse={browse} /> : null}

        {step === "review" && preview ? (
          <ReviewStep
            preview={preview}
            locale={locale}
            currentWorkCount={activeGeneration?.workCount ?? null}
            starting={start.isPending}
            startError={start.error}
            onBrowse={browse}
            onStart={() => start.mutate(preview.accessHandle)}
          />
        ) : null}

        {step === "run" && current ? (
          <div className="import-operation-anchor" ref={operationRef}>
            {finished ? (
              <ImportResult
                progress={current}
                locale={locale}
                elapsedSeconds={elapsedSeconds}
                packageName={activeGeneration?.packageName ?? ""}
                onImportAnother={restart}
                onOpenCatalog={onOpenCatalog}
              />
            ) : stopped ? (
              <ImportStopped
                progress={current}
                locale={locale}
                remainingWorkCount={activeGeneration?.workCount ?? null}
                retrying={current.operationKind === "activation" ? activate.isPending : start.isPending}
                onLeave={current.operationKind === "activation" ? dismissOperation : browse}
                onRetry={current.operationKind === "activation"
                  ? activate.variables ? () => activate.mutate(activate.variables) : undefined
                  : preview ? () => start.mutate(preview.accessHandle) : undefined}
              />
            ) : (
              <ImportRun
                progress={current}
                locale={locale}
                elapsedSeconds={elapsedSeconds}
                cancelling={cancel.isPending}
                cancelError={cancel.error}
                onCancel={() => cancel.mutate(current.operationId)}
              />
            )}
          </div>
        ) : null}

        <InstalledCatalogs
          generations={installed}
          locale={locale}
          loading={generations.isPending}
          busy={active || activate.isPending}
          activationError={activate.error}
          removalTarget={removalTarget}
          removing={remove.isPending}
          removalError={remove.error}
          onActivate={(id) => activate.mutate(id)}
          onRequestRemove={(generation) => {
            remove.reset();
            setRemovalTarget(generation);
          }}
          onCancelRemove={() => {
            remove.reset();
            setRemovalTarget(null);
          }}
          onRemove={(id) => remove.mutate(id)}
        />
      </div>
    </main>
  );
}

function useImportElapsed(progress: CatalogImportProgress | null | undefined): number | null {
  const [elapsed, setElapsed] = useState<number | null>(null);
  const operationRef = useRef<string | null>(null);
  const startedAtRef = useRef(0);
  const operationId = progress?.operationId ?? null;
  const terminal = catalogImportIsTerminal(progress?.stage);

  useEffect(() => {
    if (operationId === null) {
      operationRef.current = null;
      setElapsed(null);
      return;
    }
    if (operationRef.current !== operationId) {
      operationRef.current = operationId;
      startedAtRef.current = Date.now();
      setElapsed(terminal ? null : 0);
    }
    if (terminal) return;
    const read = () => setElapsed(Math.floor((Date.now() - startedAtRef.current) / 1000));
    read();
    const timer = setInterval(read, 1000);
    return () => clearInterval(timer);
  }, [operationId, terminal]);

  return elapsed;
}

function ImportStepper({ step, complete }: { step: ImportStep; complete: boolean }) {
  const { t } = usePresentation();
  const order: ImportStep[] = ["choose", "review", "run"];
  const currentIndex = order.indexOf(step);
  const labels = {
    choose: t("import.step.choose"),
    review: t("import.step.review"),
    run: t("import.step.run"),
  } as const;

  return (
    <ol className="import-stepper">
      {order.map((value, index) => {
        const done = complete || index < currentIndex;
        return (
          <li
            className={`import-step${index === currentIndex && !complete ? " is-current" : ""}${done ? " is-done" : ""}`}
            aria-current={index === currentIndex && !complete ? "step" : undefined}
            key={value}
          >
            <span className="import-step-dot">
              {done ? <Check aria-hidden="true" /> : index + 1}
            </span>
            <span>{labels[value]}</span>
          </li>
        );
      })}
    </ol>
  );
}

function CatalogActivationStatus({ stage }: { stage: CatalogImportProgress["stage"] }) {
  const { t } = usePresentation();
  const key = stage === "completed"
    ? "import.activationDone"
    : stage === "cancelled" || stage === "failed"
      ? "import.activationStopped"
      : "import.activation";
  return <span className={`import-operation-badge is-${stage}`}>{t(key)}</span>;
}

function ChooseStep({ busy, onBrowse }: { busy: boolean; onBrowse: () => void }) {
  const { t } = usePresentation();
  return (
    <button
      className="import-drop"
      type="button"
      aria-label={t("import.choosePackage")}
      disabled={busy}
      onClick={onBrowse}
    >
      <span className="import-drop-icon">
        {busy ? <LoaderCircle className="import-spin" aria-hidden="true" /> : <PackageOpen aria-hidden="true" />}
      </span>
      <strong>{t(busy ? "import.opening" : "import.dropTitle")}</strong>
      <span className="import-drop-hint">{t(busy ? "import.openingHint" : "import.dropHint")}</span>
      <small>{t("import.dropSafety")}</small>
      <span className="import-drop-cta">{t("import.choosePackage")}</span>
    </button>
  );
}

function ReviewStep({
  preview,
  locale,
  currentWorkCount,
  starting,
  startError,
  onBrowse,
  onStart,
}: {
  preview: CatalogImportPreview;
  locale: string;
  currentWorkCount: number | null;
  starting: boolean;
  startError: Error | null;
  onBrowse: () => void;
  onStart: () => void;
}) {
  const { t } = usePresentation();
  const manifest = preview.manifest;
  const incoming = manifest.counts.uniqueWorks;
  const added = currentWorkCount === null ? null : incoming - currentWorkCount;
  const diskOk = preview.availableDiskBytes === 0
    || preview.availableDiskBytes >= preview.requiredDiskBytes;

  return (
    <section className={`import-preview import-panel${preview.compatible ? "" : " is-blocked"}`}>
      <span className={`import-tab${preview.compatible ? "" : " is-error"}`}>
        {t(preview.compatible ? "import.step.review" : "import.cannotImport")}
        <small>{t("import.stepOf", { index: 2, total: 3 })}</small>
      </span>

      <header className="import-package">
        <span className="import-package-icon"><Database aria-hidden="true" /></span>
        <span className="import-package-copy">
          <strong>{preview.displayName}</strong>
          <span>
            {manifest.source.name} · {formatTimestamp(manifest.createdAt, locale)} · {formatByteSize(preview.compressedBytes, locale)}
          </span>
        </span>
        <span className="import-profile-badge">{t(catalogProfileMessageKey(manifest.profile))}</span>
      </header>

      {preview.compatible ? (
        <div className="import-delta">
          {currentWorkCount === null ? (
            <span className="import-delta-side is-next">
              <small>{t("import.worksAfter")}</small>
              <b>{incoming.toLocaleString(locale)}</b>
            </span>
          ) : (
            <>
              <span className="import-delta-side">
                <small>{t("import.worksNow")}</small>
                <b>{currentWorkCount.toLocaleString(locale)}</b>
              </span>
              <ArrowRight className="import-delta-arrow" aria-hidden="true" />
              <span className="import-delta-side is-next">
                <small>{t("import.worksAfter")}</small>
                <b>{incoming.toLocaleString(locale)}</b>
              </span>
              {added !== null && added > 0 ? (
                <span className="import-delta-gain">{t("import.worksAdded", { count: added.toLocaleString(locale) })}</span>
              ) : null}
            </>
          )}
        </div>
      ) : null}

      <div className="import-outcome">
        <ImportOutcomeRow tone={diskOk ? "ok" : "error"}>
          {t("import.diskNeed", {
            required: formatByteSize(preview.requiredDiskBytes, locale),
            available: formatByteSize(preview.availableDiskBytes, locale),
          })}
        </ImportOutcomeRow>
        {preview.compatible ? (
          <ImportOutcomeRow tone="ok">{t("import.versionOk")}</ImportOutcomeRow>
        ) : (
          <ImportOutcomeRow tone="ok">{t("import.packageIntact")}</ImportOutcomeRow>
        )}
        {preview.omittedFields.length ? (
          <ImportOutcomeRow tone="warn">
            {t("import.missingFields", { count: preview.omittedFields.length })}
          </ImportOutcomeRow>
        ) : null}
        {preview.blockingIssues.map((issue) => (
          <ImportOutcomeRow tone="error" key={issue}>{issue}</ImportOutcomeRow>
        ))}
        {preview.warnings.map((warning) => (
          <ImportOutcomeRow tone="warn" key={warning}>{warning}</ImportOutcomeRow>
        ))}
      </div>

      <details className="import-technical">
        <summary><ChevronRight aria-hidden="true" />{t("import.advancedDetails")}</summary>
        <dl className="import-technical-grid">
          <ImportFact label={t("import.compressedSize")} value={formatByteSize(preview.compressedBytes, locale)} />
          <ImportFact label={t("import.uncompressedSize")} value={formatByteSize(preview.uncompressedBytes, locale)} />
          <ImportFact label={t("import.requiredSpace")} value={formatByteSize(preview.requiredDiskBytes, locale)} />
          <ImportFact label={t("import.availableSpace")} value={formatByteSize(preview.availableDiskBytes, locale)} />
          <ImportFact label={t("import.works")} value={manifest.counts.uniqueWorks.toLocaleString(locale)} />
          <ImportFact label={t("import.archives")} value={manifest.counts.roms.toLocaleString(locale)} />
          <ImportFact label={t("import.internalFiles")} value={manifest.counts.files.toLocaleString(locale)} />
          <ImportFact label={t("import.relations")} value={manifest.counts.relations.toLocaleString(locale)} />
        </dl>
        <ImportFieldList label={t("import.includedFields")} fields={manifest.fields} />
        {preview.omittedFields.length ? (
          <ImportFieldList label={t("import.omittedFields")} fields={preview.omittedFields} omitted />
        ) : null}
      </details>

      {startError ? (
        <ImportCallout
          tone="error"
          icon={<ShieldAlert />}
          text={t("import.startFailed")}
          detail={String(startError)}
        />
      ) : null}

      <footer className="import-actions">
        <span className="import-safety">
          <ShieldCheck aria-hidden="true" />
          {t(preview.compatible ? "import.atomicHelp" : "import.blockedSafety")}
        </span>
        <span className="import-pair">
          <button className="import-pair-secondary" type="button" disabled={starting} onClick={onBrowse}>
            {t("import.chooseAnother")}
          </button>
          <button
            className="import-pair-primary"
            type="button"
            disabled={!preview.compatible || starting}
            onClick={onStart}
          >
            {starting ? <LoaderCircle className="import-spin" aria-hidden="true" /> : <PackageOpen aria-hidden="true" />}
            {t(starting ? "import.starting" : "import.start")}
          </button>
        </span>
      </footer>
    </section>
  );
}

function ImportRun({
  progress,
  locale,
  elapsedSeconds,
  cancelling,
  cancelError,
  onCancel,
}: {
  progress: CatalogImportProgress;
  locale: string;
  elapsedSeconds: number | null;
  cancelling: boolean;
  cancelError: Error | null;
  onCancel: () => void;
}) {
  const { t } = usePresentation();
  const phase = catalogImportPhase(progress.stage);
  const phaseIndex = catalogImportPhases.indexOf(phase);
  const activation = progress.operationKind === "activation";
  const percent = activation ? null : catalogImportPercent(progress);
  const { processedBytes, totalBytes, uniqueWorks, relations } = progress.counters;
  const linking = phase === "details" || phase === "finishing";
  const runTitle = activation
    ? t(progress.stage === "queued" ? "import.activationPreparing" : "import.activationWorking")
    : t(phaseLabels[phase]);

  return (
    <section className="import-run import-panel" aria-label={t(activation ? "import.activation" : "import.operation")}>
      <span className="import-tab">
        {t(activation ? "import.activation" : "import.step.run")}
        {activation ? null : <small>{t("import.stepOf", { index: 3, total: 3 })}</small>}
      </span>

      <div className="import-run-head">
        <strong>{runTitle}</strong>
        {percent === null ? (
          <span className="import-percent-idle">
            {t(phase === "finishing" ? "import.almostDone" : "import.working")}
          </span>
        ) : (
          <span className="import-percent">{percent}%</span>
        )}
      </div>

      <div
        className={`import-run-bar${percent === null ? " is-indeterminate" : ""}`}
        role="progressbar"
        aria-label={t(activation ? "import.activationProgress" : "import.progress")}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent ?? undefined}
      >
        <i style={percent === null ? undefined : { width: `${percent}%` }} />
      </div>

      <div className="import-run-metrics">
        <span>
          <small>{t(activation ? "import.metricWorksAvailable" : "import.metricWorksRead")}</small>
          <b>{uniqueWorks > 0 ? uniqueWorks.toLocaleString(locale) : "—"}</b>
        </span>
        {activation ? null : (
          <span>
            <small>{t(linking ? "import.metricRelations" : "import.metricProgress")}</small>
            <b>
              {linking
                ? relations > 0 ? relations.toLocaleString(locale) : "—"
                : processedBytes > 0 && totalBytes > 0
                  ? `${formatByteSize(processedBytes, locale)} / ${formatByteSize(totalBytes, locale)}`
                  : "—"}
            </b>
          </span>
        )}
        <span>
          <small>{t("import.metricElapsed")}</small>
          <b>{elapsedSeconds === null ? "—" : formatDuration(elapsedSeconds * 1000, t)}</b>
        </span>
      </div>

      {activation ? null : (
        <ol className="import-phases">
          {catalogImportPhases.map((value, index) => (
            <li
              className={`import-phase${index === phaseIndex ? " is-current" : ""}${index < phaseIndex ? " is-done" : ""}`}
              key={value}
            >
              <span className="import-phase-bar" aria-hidden="true" />
              <span>{t(phaseLabels[value])}</span>
            </li>
          ))}
        </ol>
      )}

      {cancelError ? (
        <ImportCallout tone="error" icon={<ShieldAlert />} text={t("common.requestFailed", { error: String(cancelError) })} />
      ) : null}

      <footer className="import-actions">
        <span className="import-safety">
          <ShieldCheck aria-hidden="true" />
          {t(activation
            ? "import.activationSafety"
            : phase === "finishing" ? "import.finishSafety" : "import.cancelSafety")}
        </span>
        <button className="import-button import-button-danger" type="button" disabled={cancelling} onClick={onCancel}>
          {cancelling ? <LoaderCircle className="import-spin" aria-hidden="true" /> : <X aria-hidden="true" />}
          {t("import.cancel")}
        </button>
      </footer>
    </section>
  );
}

function ImportResult({
  progress,
  locale,
  elapsedSeconds,
  packageName,
  onImportAnother,
  onOpenCatalog,
}: {
  progress: CatalogImportProgress;
  locale: string;
  elapsedSeconds: number | null;
  packageName: string;
  onImportAnother: () => void;
  onOpenCatalog?: () => void;
}) {
  const { t } = usePresentation();
  const activation = progress.operationKind === "activation";
  return (
    <section className="import-result import-panel">
      <div role="status">
        <span className="import-tab is-success">
          {t(activation ? "import.activationDone" : "import.done")}
          {elapsedSeconds === null ? null : <small>{formatDuration(elapsedSeconds * 1000, t)}</small>}
        </span>

        <p className="import-result-figure">
          <b>{progress.counters.uniqueWorks.toLocaleString(locale)}</b>
          <span>{t("import.doneCaption")}</span>
        </p>
        <p className="import-result-help">{t("import.doneHelp")}</p>
      </div>

      <footer className="import-actions">
        <span className="import-safety">
          <CheckCircle2 aria-hidden="true" />
          {packageName ? <strong>{packageName}</strong> : t("import.atomicHelp")}
        </span>
        {onOpenCatalog ? (
          <span className="import-pair">
            <button className="import-pair-secondary" type="button" onClick={onImportAnother}>
              {t(activation ? "import.choosePackage" : "import.importAnother")}
            </button>
            <button className="import-pair-primary" type="button" onClick={onOpenCatalog}>
              <ArrowRight aria-hidden="true" />{t("import.browseCatalog")}
            </button>
          </span>
        ) : (
          <button className="import-button" type="button" onClick={onImportAnother}>
            {t(activation ? "import.choosePackage" : "import.importAnother")}
          </button>
        )}
      </footer>
    </section>
  );
}

function ImportStopped({
  progress,
  locale,
  remainingWorkCount,
  retrying,
  onLeave,
  onRetry,
}: {
  progress: CatalogImportProgress;
  locale: string;
  remainingWorkCount: number | null;
  retrying: boolean;
  onLeave: () => void;
  onRetry?: () => void;
}) {
  const { t } = usePresentation();
  const activation = progress.operationKind === "activation";

  return (
    <section className="import-result import-panel is-blocked">
      <div role="alert">
        <span className="import-tab is-error">
          {t(activation ? "import.activationStopped" : "import.stopped")}
        </span>

        <p className="import-result-figure">
          <b>0</b>
          <span>{t("import.noChangesCaption")}</span>
        </p>
        <p className="import-result-help">
          {t(activation ? "import.activationStoppedHelp" : "import.stoppedHelp")}
        </p>
      </div>
      {progress.detail ? (
        <details className="import-technical import-stopped-technical">
          <summary><ChevronRight aria-hidden="true" />{t("import.advancedDetails")}</summary>
          <p className="import-detail-line">{t("common.technicalDetail", { detail: progress.detail })}</p>
        </details>
      ) : null}

      <footer className="import-actions">
        <span className="import-safety">
          <ShieldCheck aria-hidden="true" />
          {remainingWorkCount === null
            ? t("import.blockedSafety")
            : t("import.worksStillAvailable", { count: remainingWorkCount.toLocaleString(locale) })}
        </span>
        {onRetry ? (
          <span className="import-pair">
            <button className="import-pair-secondary" type="button" disabled={retrying} onClick={onLeave}>
              {t(activation ? "common.back" : "import.chooseAnother")}
            </button>
            <button
              className="import-pair-primary"
              type="button"
              disabled={retrying}
              onClick={onRetry}
            >
              {retrying ? <LoaderCircle className="import-spin" aria-hidden="true" /> : <PackageOpen aria-hidden="true" />}
              {t(retrying ? "import.starting" : "import.tryAgain")}
            </button>
          </span>
        ) : (
          <button className="import-button" type="button" onClick={onLeave}>
            {t(activation ? "common.back" : "import.chooseAnother")}
          </button>
        )}
      </footer>
    </section>
  );
}

function InstalledCatalogs({
  generations,
  locale,
  loading,
  busy,
  activationError,
  removalTarget,
  removing,
  removalError,
  onActivate,
  onRequestRemove,
  onCancelRemove,
  onRemove,
}: {
  generations: CatalogGenerationSummary[];
  locale: string;
  loading: boolean;
  busy: boolean;
  activationError: Error | null;
  removalTarget: CatalogGenerationSummary | null;
  removing: boolean;
  removalError: Error | null;
  onActivate: (generationId: string) => void;
  onRequestRemove: (generation: CatalogGenerationSummary) => void;
  onCancelRemove: () => void;
  onRemove: (generationId: string) => void;
}) {
  const { t } = usePresentation();
  const totalBytes = generations.reduce((sum, generation) => sum + generation.databaseBytes, 0);

  return (
    <section className="import-catalogs" aria-label={t("import.catalogs")}>
      <header className="import-catalogs-head">
        <div>
          <h2>{t("import.catalogs")}</h2>
          <p>{t("import.catalogsHelp")}</p>
        </div>
        {generations.length ? (
          <span className="import-disk-total">
            {t("import.diskTotal", { count: generations.length, size: formatByteSize(totalBytes, locale) })}
          </span>
        ) : null}
      </header>

      {activationError ? (
        <ImportCallout tone="error" icon={<ShieldAlert />} text={t("common.requestFailed", { error: String(activationError) })} />
      ) : null}
      {removalError ? (
        <ImportCallout tone="error" icon={<ShieldAlert />} text={t("common.requestFailed", { error: String(removalError) })} />
      ) : null}

      {loading ? (
        <ul className="import-catalog-list" aria-label={t("import.loadingHistory")}>
          {[0, 1, 2].map((row) => (
            <li className="import-skeleton" key={row} aria-hidden="true">
              <span>
                <span className="import-shimmer" />
                <span className="import-shimmer" />
              </span>
              <span className="import-shimmer" />
              <span className="import-shimmer" />
            </li>
          ))}
        </ul>
      ) : generations.length === 0 ? (
        <p className="import-catalogs-state">{t("import.emptyHelp")}</p>
      ) : (
        <ul className="import-catalog-list">
          {generations.map((generation) => {
            const profile = t(catalogProfileMessageKey(generation.profile));
            const hasPackageName = generation.packageName.trim().length > 0;
            const importedAt = formatTimestamp(generation.importedAt, locale);
            const kind = t(generationKindMessageKey(generation.kind));
            const label = hasPackageName
              ? generation.packageName
              : generation.kind === "embedded"
                ? t("import.builtInCatalog")
                : `${kind} · ${importedAt}`;
            const metadata = generation.kind === "embedded"
              ? [profile]
              : [generation.sourceName, profile, ...(hasPackageName ? [kind, importedAt] : [])];
            const confirmingRemoval = removalTarget?.id === generation.id;
            const actionsBusy = busy || removalTarget !== null;
            const rowState = generation.state === "active"
              ? " is-active"
              : generation.state === "failed"
                ? " is-failed"
                : generation.kind === "embedded"
                  ? " is-embedded"
                  : "";
            return (
              <li className={`import-catalog${rowState}`} key={generation.id}>
                <span className="import-catalog-name">
                  <strong>{label}</strong>
                  <span>
                    {metadata.join(" · ")} · {generation.workCount.toLocaleString(locale)} {t("import.works").toLocaleLowerCase(locale)}
                  </span>
                </span>
                <span className="import-catalog-size"><HardDrive aria-hidden="true" />{formatByteSize(generation.databaseBytes, locale)}</span>
                <span className="import-catalog-actions">
                  {generation.state === "active" ? (
                    <span className="import-catalog-current">{t("import.active")}</span>
                  ) : (
                    <button
                      className="import-catalog-action"
                      type="button"
                      disabled={actionsBusy || generation.state === "failed"}
                      onClick={() => onActivate(generation.id)}
                    >
                      {t("import.activate")}
                    </button>
                  )}
                  {generation.kind === "imported" && generation.state !== "active" ? (
                    <button
                      className="import-catalog-action is-danger"
                      type="button"
                      disabled={actionsBusy}
                      onClick={() => onRequestRemove(generation)}
                    >
                      <Trash2 aria-hidden="true" />{t("import.remove")}
                    </button>
                  ) : null}
                </span>
                {generation.failureDetail ? (
                  <span className="import-catalog-failure">{t("common.technicalDetail", { detail: generation.failureDetail })}</span>
                ) : null}
                {confirmingRemoval ? (
                  <div className="import-remove-confirm" role="alert">
                    <span>
                      <strong>{t("import.removeConfirmTitle", { name: label })}</strong>
                      <small>{t("import.removeConfirmHelp", { size: formatByteSize(generation.databaseBytes, locale) })}</small>
                    </span>
                    <button className="import-button" type="button" disabled={removing} onClick={onCancelRemove}>
                      {t("import.removeKeep")}
                    </button>
                    <button
                      className="import-button import-button-solid-danger"
                      type="button"
                      disabled={busy || removing}
                      onClick={() => onRemove(generation.id)}
                    >
                      {removing ? <LoaderCircle className="import-spin" aria-hidden="true" /> : <Trash2 aria-hidden="true" />}
                      {t(removing ? "import.removing" : "import.remove")}
                    </button>
                  </div>
                ) : null}
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}

function ImportOutcomeRow({ tone, children }: { tone: "ok" | "warn" | "error"; children: ReactNode }) {
  return (
    <p className={`import-outcome-row is-${tone}`}>
      {tone === "ok" ? <Check aria-hidden="true" /> : <AlertTriangle aria-hidden="true" />}
      {children}
    </p>
  );
}

function ImportFact({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <dt>{label}</dt>
      <dd>{value}</dd>
    </div>
  );
}

function ImportFieldList({ label, fields, omitted = false }: { label: string; fields: string[]; omitted?: boolean }) {
  return (
    <div className="import-field-list">
      <p>{label}</p>
      <div>
        {fields.map((field) => (
          <span className={`import-field${omitted ? " is-omitted" : ""}`} key={field}>{field}</span>
        ))}
      </div>
    </div>
  );
}

function ImportCallout({
  tone,
  icon,
  text,
  detail,
}: {
  tone: "warning" | "error";
  icon: ReactNode;
  text: string;
  detail?: string;
}) {
  const { t } = usePresentation();
  return (
    <div className={`import-callout import-callout-${tone}`} role={tone === "error" ? "alert" : undefined}>
      {icon}
      <div>
        <span>{text}</span>
        {detail ? <small>{t("common.technicalDetail", { detail })}</small> : null}
      </div>
    </div>
  );
}

export { formatByteSize } from "../../formatByteSize";

function formatTimestamp(value: string, locale: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString(locale);
}

async function refreshAfterCatalogOperation(queryClient: ReturnType<typeof useQueryClient>): Promise<void> {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: generationKey }),
    queryClient.invalidateQueries({ queryKey: ["catalog"] }),
    queryClient.invalidateQueries({ queryKey: ["search"] }),
  ]);
}
