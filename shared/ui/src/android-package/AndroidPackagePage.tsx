import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  BookPlus,
  Check,
  FileKey,
  LoaderCircle,
  Library as LibraryIcon,
  PackageOpen,
  RotateCcw,
  Settings,
  ShieldCheck,
  Smartphone,
  X,
} from "lucide-react";

import { formatByteSize } from "../formatByteSize";
import { usePresentation } from "../preferences/PresentationProvider";
import type {
  AndroidAppGateway,
  AndroidAppView,
  AndroidPackageBlockReason,
  AndroidPackageGateway,
  AndroidPackageInstallState,
  AndroidPackageState,
} from "./types";

const stateKey = ["android-package", "state"] as const;
const associationsKey = ["library", "android-apps"] as const;

export function AndroidPackagePage({
  gateway,
  associationGateway,
  workCode,
  onOpenLibrary,
}: {
  gateway: AndroidPackageGateway;
  associationGateway?: AndroidAppGateway;
  workCode?: string;
  onOpenLibrary?: () => void;
}) {
  const { locale, t } = usePresentation();
  const queryClient = useQueryClient();
  const state = useQuery({
    queryKey: stateKey,
    queryFn: () => gateway.readState(),
    refetchInterval: (query) => {
      const status = query.state.data?.installStatus?.state;
      return status === "preparing" || status === "awaiting_user_confirmation" ? 750 : false;
    },
  });
  const update = (next: AndroidPackageState) => queryClient.setQueryData(stateKey, next);
  const select = useMutation({ mutationFn: () => gateway.selectAndInspect(), onSuccess: update });
  const clear = useMutation({ mutationFn: () => gateway.clearSelection(), onSuccess: update });
  const approve = useMutation({ mutationFn: () => gateway.openSourceApproval(), onSuccess: update });
  const install = useMutation({ mutationFn: () => gateway.requestInstall(), onSuccess: update });
  const associations = useQuery({
    queryKey: associationsKey,
    queryFn: () => associationGateway?.list() ?? Promise.resolve([]),
    enabled: Boolean(associationGateway && workCode),
  });
  const associate = useMutation({
    mutationFn: () => {
      if (!associationGateway || !workCode) throw new Error("Android app association is unavailable");
      return associationGateway.associateInstalled(workCode);
    },
    onSuccess: (associated) => {
      queryClient.setQueryData<AndroidAppView[]>(associationsKey, (current) => [
        associated,
        ...(current ?? []).filter((item) => item.association.id !== associated.association.id
          && item.association.workCode.toLocaleLowerCase() !== associated.association.workCode.toLocaleLowerCase()),
      ]);
    },
  });
  const current = state.data;
  const inspection = current?.inspection;
  const status = current?.installStatus;
  const pending = select.isPending || clear.isPending || approve.isPending || install.isPending
    || associate.isPending;
  const installPending = status?.state === "preparing" || status?.state === "awaiting_user_confirmation";
  const error = state.error ?? select.error ?? clear.error ?? approve.error ?? install.error
    ?? associations.error ?? associate.error;
  const existingAssociation = associations.data?.find((item) => (
    item.association.workCode.toLocaleLowerCase() === workCode?.toLocaleLowerCase()
  ));

  return (
    <main className="android-package-page">
      <header className="android-package-masthead">
        <span>{t("androidPackage.eyebrow")}</span>
        <h1>{t("androidPackage.title")}</h1>
        <p>{t("androidPackage.description")}</p>
      </header>

      {state.isPending ? (
        <section className="android-package-panel android-package-loading" aria-live="polite">
          <LoaderCircle aria-hidden="true" />
          <p>{t("androidPackage.loading")}</p>
        </section>
      ) : !current ? (
        <section className="android-package-panel android-package-unavailable" role="alert">
          <span className="android-package-panel-mark"><X aria-hidden="true" /></span>
          <div>
            <h2>{t("androidPackage.actionFailed")}</h2>
            <p>{String(error ?? t("androidPackage.stateUnavailable"))}</p>
            <button
              className="android-package-primary"
              type="button"
              onClick={() => void state.refetch()}
            >
              <RotateCcw aria-hidden="true" />
              {t("androidPackage.retry")}
            </button>
          </div>
        </section>
      ) : current.capability.status === "unavailable" ? (
        <section className="android-package-panel android-package-unavailable">
          <span className="android-package-panel-mark"><Smartphone aria-hidden="true" /></span>
          <div>
            <h2>{t("androidPackage.unavailableTitle")}</h2>
            <p>{t("androidPackage.unavailableHelp")}</p>
          </div>
        </section>
      ) : (
        <section className="android-package-panel" aria-labelledby="android-package-panel-title">
          <header className="android-package-panel-head">
            <span className="android-package-panel-mark"><PackageOpen aria-hidden="true" /></span>
            <div>
              <h2 id="android-package-panel-title">
                {inspection ? t("androidPackage.reviewTitle") : t("androidPackage.chooseTitle")}
              </h2>
              <p>{inspection ? t("androidPackage.reviewHelp") : t("androidPackage.chooseHelp")}</p>
              {workCode ? (
                <small className="android-package-work-context">
                  {t("androidApp.forWork", { code: workCode })}
                </small>
              ) : null}
            </div>
          </header>

          {!inspection ? (
            <button
              className="android-package-primary"
              type="button"
              disabled={pending}
              onClick={() => select.mutate()}
            >
              {select.isPending ? <LoaderCircle className="android-package-spin" aria-hidden="true" /> : <PackageOpen aria-hidden="true" />}
              {t(select.isPending ? "androidPackage.inspecting" : "androidPackage.choose")}
            </button>
          ) : (
            <>
              <div className="android-package-identity">
                <span className="android-package-app-icon"><Smartphone aria-hidden="true" /></span>
                <div>
                  <small>{inspection.displayName}</small>
                  <h3>{inspection.applicationLabel}</h3>
                  <p>
                    {t("androidPackage.version", {
                      version: inspection.versionName ?? inspection.versionCode,
                    })}
                    <span aria-hidden="true"> · </span>
                    {formatByteSize(inspection.sizeBytes, locale)}
                  </p>
                </div>
              </div>

              <div className="android-package-checks">
                <span className={inspection.installable ? "is-safe" : "is-blocked"}>
                  {inspection.installable ? <ShieldCheck aria-hidden="true" /> : <X aria-hidden="true" />}
                  <span>
                    <strong>{t(inspection.installable ? "androidPackage.ready" : "androidPackage.blocked")}</strong>
                    <small>
                      {inspection.blockReason
                        ? t(blockReasonKey(inspection.blockReason))
                        : t("androidPackage.readyHelp")}
                    </small>
                  </span>
                </span>
                <span className={inspection.signingCertificateSha256.length ? "is-safe" : "is-blocked"}>
                  <FileKey aria-hidden="true" />
                  <span>
                    <strong>{t(inspection.signingCertificateSha256.length ? "androidPackage.signed" : "androidPackage.signatureMissing")}</strong>
                    <small>{t(inspection.signingCertificateSha256.length ? "androidPackage.signedHelp" : "androidPackage.block.missingSignature")}</small>
                  </span>
                </span>
              </div>

              <p className="android-package-caution">{t("androidPackage.safetyCaution")}</p>

              <details className="android-package-details">
                <summary>{t("androidPackage.fileDetails")}</summary>
                <dl>
                  <div><dt>{t("androidPackage.packageName")}</dt><dd>{inspection.packageName}</dd></div>
                  <div><dt>{t("androidPackage.androidVersions")}</dt><dd>{sdkRange(inspection.minimumSdk, inspection.targetSdk)}</dd></div>
                  <div><dt>SHA-256</dt><dd>{inspection.sha256}</dd></div>
                  {inspection.signingCertificateSha256.map((fingerprint) => (
                    <div key={fingerprint}>
                      <dt>{t("androidPackage.signingCertificate")}</dt>
                      <dd>{fingerprint}</dd>
                    </div>
                  ))}
                </dl>
              </details>

              {current.capability.status === "approval_required" ? (
                <div className="android-package-approval">
                  <div>
                    <Settings aria-hidden="true" />
                    <span>
                      <strong>{t("androidPackage.approvalTitle")}</strong>
                      <small>{t("androidPackage.approvalHelp")}</small>
                    </span>
                  </div>
                  <button type="button" disabled={pending} onClick={() => approve.mutate()}>
                    {approve.isPending ? <LoaderCircle className="android-package-spin" aria-hidden="true" /> : <Settings aria-hidden="true" />}
                    {t("androidPackage.openSettings")}
                  </button>
                </div>
              ) : null}

              {status ? <InstallStatus state={status.state} detail={status.technicalDetail} /> : null}

              {status?.state === "installed" ? (
                <AndroidLibraryAssociation
                  workCode={workCode}
                  currentPackageName={inspection.packageName}
                  existing={existingAssociation}
                  pending={associate.isPending || associations.isPending}
                  onAssociate={() => associate.mutate()}
                  onOpenLibrary={onOpenLibrary}
                />
              ) : null}

              <div className="android-package-actions">
                <button
                  type="button"
                  disabled={pending || installPending}
                  onClick={() => select.mutate()}
                >
                  <RotateCcw aria-hidden="true" />
                  {t("androidPackage.chooseAnother")}
                </button>
                <button
                  type="button"
                  disabled={pending || installPending}
                  onClick={() => clear.mutate()}
                >
                  <X aria-hidden="true" />
                  {t(status?.state === "installed" ? "androidPackage.clear" : "common.cancel")}
                </button>
                {status?.state !== "installed" ? (
                  <button
                    className="android-package-primary"
                    type="button"
                    disabled={pending || installPending || !inspection.installable || current.capability.status !== "ready"}
                    onClick={() => install.mutate()}
                  >
                    {install.isPending || installPending
                      ? <LoaderCircle className="android-package-spin" aria-hidden="true" />
                      : <PackageOpen aria-hidden="true" />}
                    {t(installPending ? "androidPackage.waiting" : "androidPackage.continue")}
                  </button>
                ) : null}
              </div>
            </>
          )}

          {error ? (
            <div className="android-package-error" role="alert">
              <strong>{t("androidPackage.actionFailed")}</strong>
              <details><summary>{t("androidPackage.errorDetails")}</summary><p>{String(error)}</p></details>
            </div>
          ) : null}
        </section>
      )}
    </main>
  );
}

function AndroidLibraryAssociation({
  workCode,
  currentPackageName,
  existing,
  pending,
  onAssociate,
  onOpenLibrary,
}: {
  workCode?: string;
  currentPackageName: string;
  existing?: AndroidAppView;
  pending: boolean;
  onAssociate: () => void;
  onOpenLibrary?: () => void;
}) {
  const { t } = usePresentation();
  if (!workCode) {
    return (
      <div className="android-package-library-note">
        <LibraryIcon aria-hidden="true" />
        <span>
          <strong>{t("androidApp.notLinkedTitle")}</strong>
          <small>{t("androidApp.notLinkedHelp")}</small>
        </span>
      </div>
    );
  }
  const linked = existing?.association.packageName === currentPackageName;
  return (
    <div className={`android-package-library-note${linked ? " is-linked" : ""}`}>
      {linked ? <Check aria-hidden="true" /> : <BookPlus aria-hidden="true" />}
      <span>
        <strong>{t(linked ? "androidApp.linkedTitle" : "androidApp.linkTitle")}</strong>
        <small>{t(linked ? "androidApp.linkedHelp" : "androidApp.linkHelp", { code: workCode })}</small>
      </span>
      {linked && onOpenLibrary ? (
        <button type="button" onClick={onOpenLibrary}>
          <LibraryIcon aria-hidden="true" />{t("androidApp.openLibrary")}
        </button>
      ) : !linked ? (
        <button type="button" disabled={pending} onClick={onAssociate}>
          {pending ? <LoaderCircle className="android-package-spin" aria-hidden="true" /> : <BookPlus aria-hidden="true" />}
          {t(existing ? "androidApp.replaceLink" : "androidApp.addToLibrary")}
        </button>
      ) : null}
    </div>
  );
}

function InstallStatus({ state, detail }: { state: AndroidPackageInstallState; detail: string | null }) {
  const { t } = usePresentation();
  const pending = state === "preparing" || state === "awaiting_user_confirmation";
  const success = state === "installed";
  const warning = state === "approval_required" || state === "cancelled";
  return (
    <div className={`android-package-status is-${state}`} role={state === "failed" ? "alert" : "status"}>
      {pending
        ? <LoaderCircle className="android-package-spin" aria-hidden="true" />
        : success
          ? <Check aria-hidden="true" />
          : warning
            ? <AlertTriangle aria-hidden="true" />
            : <X aria-hidden="true" />}
      <span>
        <strong>{t(installStateKey(state))}</strong>
        {state === "awaiting_user_confirmation" ? <small>{t("androidPackage.confirmOnAndroid")}</small> : null}
        {detail ? <small>{t("common.technicalDetail", { detail })}</small> : null}
      </span>
    </div>
  );
}

function blockReasonKey(reason: AndroidPackageBlockReason) {
  switch (reason) {
    case "incompatible_sdk": return "androidPackage.block.incompatibleSdk" as const;
    case "split_package": return "androidPackage.block.splitPackage" as const;
    case "self_update": return "androidPackage.block.selfUpdate" as const;
    case "missing_signature": return "androidPackage.block.missingSignature" as const;
  }
}

function installStateKey(state: AndroidPackageInstallState) {
  switch (state) {
    case "approval_required": return "androidPackage.status.approvalRequired" as const;
    case "preparing": return "androidPackage.status.preparing" as const;
    case "awaiting_user_confirmation": return "androidPackage.status.awaiting" as const;
    case "installed": return "androidPackage.status.installed" as const;
    case "cancelled": return "androidPackage.status.cancelled" as const;
    case "failed": return "androidPackage.status.failed" as const;
  }
}

function sdkRange(minimum: number | null, target: number | null): string {
  const minimumLabel = minimum === null ? "?" : minimum.toString();
  const targetLabel = target === null ? "?" : target.toString();
  return `API ${minimumLabel} → ${targetLabel}`;
}
