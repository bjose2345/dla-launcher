import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  CheckCircle2,
  Download,
  Languages,
  LoaderCircle,
  Lock,
  LogIn,
  Pause,
  Play,
  RefreshCw,
} from "lucide-react";
import { useEffect, useState } from "react";

import { ProgressBar, timeLeftText } from "../../app/ProgressBar";
import { DeliverySignIn } from "../delivery/DeliverySignIn";
import { patronPlansUrl } from "../delivery/deliveryText";
import type { DeliveryGateway } from "../delivery/types";
import { acceptDeliveryView } from "../delivery/useDelivery";
import { usePresentation } from "../../preferences/PresentationProvider";
import { archivePackKey } from "./archiveKeys";
import type { UntranslatableLanguage } from "./language";
import { madeDate } from "./TranslatedWorks";
import { TranslatorHelpDialog } from "./TranslatorHelpDialog";
import type {
  GameTranslationView,
  PackView,
  PreparationProblem,
  PreparationView,
  TextGroup,
  TranslationGateway,
  TranslationSettings,
} from "./types";
import { Switch } from "../../app/Switch";

export const gameTranslationKey = (installationId: string) =>
  ["translation", "game", installationId] as const;
export const translationSettingsKey = ["translation", "settings"] as const;

const groupLabelKeys = {
  menus: "translation.group.menus",
  names_and_descriptions: "translation.group.namesAndDescriptions",
  dialogue: "translation.group.dialogue",
} as const satisfies Record<TextGroup, string>;

const problemKeys = {
  translator_unreachable: "translation.problem.unreachable",
  translator_refused: "translation.problem.refused",
  translator_reply: "translation.problem.reply",
  game_files: "translation.problem.gameFiles",
  other: "translation.problem.other",
} as const satisfies Record<PreparationProblem, string>;

const untranslatableKeys = {
  includes_english: "translation.includesEnglish",
  not_japanese: "translation.notJapanese",
} as const satisfies Record<UntranslatableLanguage, string>;

/**
 * The Translation tab of a game's Edit details: turns the launcher's
 * translation plugin on, shows how far the game is prepared, and removes it.
 * A work the catalog lists in English, or not in Japanese, says why instead
 * of offering it; one already translated keeps its controls. The Archive's
 * translation is offered either way: it needs no translator of the player's
 * own.
 */
export function GameTranslationTab({
  installationId,
  gateway,
  deliveryGateway,
  untranslatable = null,
  onOpenSettings,
  onOpenExternal,
  onPlay,
}: {
  installationId: string;
  gateway: TranslationGateway;
  /** Signs in to DLA, which the Archive's translation needs. */
  deliveryGateway?: DeliveryGateway;
  untranslatable?: UntranslatableLanguage | null;
  onOpenSettings?: () => void;
  onOpenExternal?: (url: string) => Promise<void>;
  onPlay?: () => void;
}) {
  const { t } = usePresentation();
  const queryClient = useQueryClient();
  const key = gameTranslationKey(installationId);
  const view = useQuery({ queryKey: key, queryFn: () => gateway.readGame(installationId) });
  const settings = useQuery({ queryKey: translationSettingsKey, queryFn: () => gateway.readSettings() });
  const [help, setHelp] = useState(false);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const [signingIn, setSigningIn] = useState(false);

  useEffect(() => gateway.subscribe((next) => {
    if (next.installationId === installationId) {
      queryClient.setQueryData(gameTranslationKey(installationId), next);
    }
  }), [gateway, installationId, queryClient]);

  const accept = (next: GameTranslationView) => queryClient.setQueryData(key, next);
  const turnOn = useMutation({ mutationFn: () => gateway.turnOn(installationId), onSuccess: accept });
  const show = useMutation({ mutationFn: (shown: boolean) => gateway.show(installationId, shown), onSuccess: accept });
  const prepare = useMutation({ mutationFn: () => gateway.prepare(installationId), onSuccess: accept });
  const pause = useMutation({ mutationFn: () => gateway.pause(installationId), onSuccess: accept });
  const again = useMutation({ mutationFn: () => gateway.translateAgain(installationId), onSuccess: accept });
  // The Archive is asked once per hour at most, and again after signing in;
  // without an answer, nothing is offered.
  useQuery({
    queryKey: archivePackKey(installationId),
    queryFn: async () => {
      queryClient.setQueryData(key, await gateway.checkPack(installationId));
      return true;
    },
    enabled: Boolean(view.data?.supported) && !untranslatable,
    staleTime: 60 * 60_000,
    retry: false,
  });
  // The Archive's translation turns translation on for the game first.
  const downloadPack = useMutation({
    mutationFn: async () => {
      if (!view.data?.on) await gateway.turnOn(installationId);
      return gateway.downloadPack(installationId);
    },
    onSuccess: accept,
  });
  const remove = useMutation({
    mutationFn: () => gateway.remove(installationId),
    onSuccess: async () => {
      setConfirmRemove(false);
      await queryClient.invalidateQueries({ queryKey: key });
    },
  });
  const idle = useMutation({
    mutationFn: (prepareOnlyWhenIdle: boolean) => gateway.saveSettings({
      translator: settings.data?.translator ?? null,
      prepareOnlyWhenIdle,
    }),
    onSuccess: (saved) => queryClient.setQueryData(translationSettingsKey, saved),
  });

  const error = view.error ?? turnOn.error ?? show.error ?? prepare.error ?? pause.error ?? again.error
    ?? remove.error ?? idle.error;
  const current = view.data;
  const busy = turnOn.isPending || show.isPending || prepare.isPending || pause.isPending || again.isPending
    || remove.isPending;
  const helpLink = (
    <button className="library-manage-link" type="button" onClick={() => setHelp(true)}>
      {t("translation.howToSetUp")}
    </button>
  );

  return (
    <div className="library-manage-stack">
      {error ? (
        <div className="app-callout" role="alert">
          <AlertTriangle aria-hidden="true" />{t("common.requestFailed", { error: String(error) })}
        </div>
      ) : null}
      {view.isPending ? (
        <p className="library-manage-note"><LoaderCircle className="app-spin" aria-hidden="true" />{t("library.loading")}</p>
      ) : null}

      {current && !current.on && current.pack && !untranslatable ? (
        <PackRow
          pack={current.pack}
          busy={downloadPack.isPending}
          error={downloadPack.error}
          onDownload={() => downloadPack.mutate()}
          onSignIn={deliveryGateway ? () => setSigningIn(true) : undefined}
          onOpenPlans={(url) => void (onOpenExternal ?? gateway.openWebsite)(url)}
        />
      ) : null}

      {current && !current.on ? (
        <section className="translation-offer">
          {current.engineLabel && !untranslatable ? (
            <span className="translation-eyebrow">{t("translation.engineJapanese", { engine: current.engineLabel })}</span>
          ) : null}
          {untranslatable ? (
            <p className="library-manage-help">{t(untranslatableKeys[untranslatable])}</p>
          ) : !current.engineLabel ? (
            <p className="library-manage-help">{t("translation.noEngine")}</p>
          ) : !current.supported ? (
            <>
              <h3>{t("translation.unsupportedTitle", { engine: current.engineLabel })}</h3>
              <p className="library-manage-help">{t("translation.unsupportedHelp")}</p>
            </>
          ) : !current.translatorReady ? (
            <>
              <h3>{t("translation.needsTranslatorTitle")}</h3>
              <p className="library-manage-help">{t("translation.needsTranslatorHelp")}</p>
              <div className="library-manage-row">
                {onOpenSettings ? (
                  <button className="app-button is-primary" type="button" onClick={onOpenSettings}>
                    {t("translation.openSettings")}
                  </button>
                ) : null}
                {helpLink}
              </div>
            </>
          ) : (
            <>
              <h3>{t("translation.offTitle")}</h3>
              <p className="library-manage-help">{t("translation.offHelp")}</p>
              <div className="library-manage-row">
                <button className="app-button is-primary" type="button" disabled={busy} onClick={() => turnOn.mutate()}>
                  {turnOn.isPending ? <LoaderCircle className="app-spin" aria-hidden="true" /> : <Languages aria-hidden="true" />}
                  {t("translation.turnOn")}
                </button>
              </div>
            </>
          )}
        </section>
      ) : null}

      {current?.on ? (
        <>
          <dl className="translation-facts">
            <dt>{t("translation.engine")}</dt>
            <dd>{current.engineLabel}</dd>
            <dt>{t("translation.language")}</dt>
            <dd>
              {t("translation.languagePair")}
              <span className="translation-chip">{t("translation.machine")}</span>
            </dd>
            <dt>{t("translation.translator")}</dt>
            <dd>
              {translatorLabel(settings.data, t)}
              {onOpenSettings ? (
                <button className="library-manage-link" type="button" onClick={onOpenSettings}>{t("translation.change")}</button>
              ) : null}
            </dd>
          </dl>

          {current.pack ? (
            <PackRow
              pack={current.pack}
              busy={downloadPack.isPending}
              error={downloadPack.error}
              onDownload={() => downloadPack.mutate()}
              onSignIn={deliveryGateway ? () => setSigningIn(true) : undefined}
              onOpenPlans={(url) => void (onOpenExternal ?? gateway.openWebsite)(url)}
            />
          ) : null}

          {current.preparation ? (
            <PreparationCard
              preparation={current.preparation}
              busy={busy}
              onPrepare={() => prepare.mutate()}
              onPause={() => pause.mutate()}
              onTranslateAgain={() => again.mutate()}
              translatingAgain={again.isPending}
              onPlay={onPlay}
              onOpenSettings={onOpenSettings}
            />
          ) : null}
          <p className="library-manage-help">
            {t(current.preparation?.state === "ready" ? "translation.updateHelp" : "translation.playAlready")}
          </p>

          <div className="translation-switches">
            <Switch
              checked={current.shown}
              disabled={busy}
              label={t("translation.show")}
              help={t("translation.showHelp")}
              onChange={(shown) => show.mutate(shown)}
            />
            <Switch
              checked={settings.data?.prepareOnlyWhenIdle ?? false}
              disabled={!settings.data || idle.isPending}
              label={t("translation.onlyWhenIdle")}
              help={t("translation.onlyWhenIdleHelp")}
              onChange={(value) => idle.mutate(value)}
            />
          </div>

          <section className="library-manage-field">
            <div className="library-manage-remove">
              {confirmRemove ? (
                <div className="library-manage-confirm" role="alertdialog" aria-modal="false" aria-label={t("translation.removeConfirm")}>
                  <span>{t("translation.removeConfirm")}</span>
                  <button className="app-button is-solid-danger" type="button" disabled={remove.isPending} onClick={() => remove.mutate()}>
                    {remove.isPending ? <LoaderCircle className="app-spin" aria-hidden="true" /> : null}
                    {t("translation.remove")}
                  </button>
                  <button className="library-manage-link" type="button" disabled={remove.isPending} onClick={() => setConfirmRemove(false)}>
                    {t("common.cancel")}
                  </button>
                </div>
              ) : (
                <button className="app-button is-danger" type="button" disabled={busy} onClick={() => setConfirmRemove(true)}>
                  {`${t("translation.remove")}…`}
                </button>
              )}
              <p className="library-manage-help">{t("translation.removeHelp")}</p>
            </div>
          </section>
        </>
      ) : null}

      {help ? <TranslatorHelpDialog gateway={gateway} onClose={() => setHelp(false)} /> : null}
      {signingIn && deliveryGateway ? (
        <DeliverySignIn
          gateway={deliveryGateway}
          onSignedIn={(next) => {
            // Asks the Archive again for this account.
            acceptDeliveryView(queryClient, next);
            setSigningIn(false);
          }}
          onClose={() => setSigningIn(false)}
          onOpenExternal={onOpenExternal}
        />
      ) : null}
    </div>
  );
}

function PackRow({
  pack,
  busy,
  error,
  onDownload,
  onSignIn,
  onOpenPlans,
}: {
  pack: PackView;
  busy: boolean;
  error: unknown;
  onDownload: () => void;
  onSignIn?: () => void;
  onOpenPlans: (url: string) => void;
}) {
  const { locale, t } = usePresentation();
  if (!pack.available && !pack.installed && !pack.access) return null;
  const summary = (lines: number, made: string) => t("translation.pack.summary", {
    lines: lines.toLocaleString(locale),
    date: madeDate(made, locale),
  });
  return (
    <section className="translation-pack">
      {pack.installed ? (
        <p className="translation-pack-installed">
          <CheckCircle2 aria-hidden="true" />
          <span>
            {t("translation.pack.installed")}
            <small>{summary(pack.installed.lines, pack.installed.made)}</small>
          </span>
        </p>
      ) : null}
      {pack.available ? (
        <>
          <p className="translation-pack-offer">
            {t(pack.installed ? "translation.pack.newer" : "translation.pack.available")}
            <small>{summary(pack.available.lines, pack.available.made)}</small>
          </p>
          <div className="library-manage-row">
            <button className="app-button is-primary" type="button" disabled={busy} onClick={onDownload}>
              {busy ? <LoaderCircle className="app-spin" aria-hidden="true" /> : <Download aria-hidden="true" />}
              {t(pack.installed ? "translation.pack.downloadNewer" : "translation.pack.download")}
            </button>
          </div>
          {pack.installed ? null : <p className="translation-pack-help">{t("translation.pack.noTranslatorNeeded")}</p>}
        </>
      ) : null}
      {pack.access === "for_elite" ? (
        <div className="translation-pack-access">
          <p className="translation-pack-offer">
            <span><Lock aria-hidden="true" />{t("translation.pack.forElite")}</span>
          </p>
          <button className="library-manage-link" type="button" onClick={() => onOpenPlans(patronPlansUrl(locale))}>
            {t("delivery.plansLink")}
          </button>
        </div>
      ) : null}
      {pack.access === "sign_in" ? (
        <div className="translation-pack-access">
          <p className="translation-pack-offer">
            <span><Lock aria-hidden="true" />{t("translation.pack.signIn")}</span>
          </p>
          {onSignIn ? (
            <div className="library-manage-row">
              <button className="app-button" type="button" onClick={onSignIn}>
                <LogIn aria-hidden="true" />
                {t("delivery.signIn")}
              </button>
            </div>
          ) : null}
          {/* Only Elite members have a DLA account. */}
          <button className="library-manage-link" type="button" onClick={() => onOpenPlans(patronPlansUrl(locale))}>
            {t("delivery.plansLink")}
          </button>
        </div>
      ) : null}
      {error ? (
        <p className="library-manage-result is-problem" role="alert">
          <AlertTriangle aria-hidden="true" />{String(error)}
        </p>
      ) : null}
    </section>
  );
}

function PreparationCard({
  preparation,
  busy,
  onPrepare,
  onPause,
  onTranslateAgain,
  translatingAgain,
  onPlay,
  onOpenSettings,
}: {
  preparation: PreparationView;
  busy: boolean;
  onPrepare: () => void;
  onPause: () => void;
  onTranslateAgain: () => void;
  translatingAgain: boolean;
  onPlay?: () => void;
  onOpenSettings?: () => void;
}) {
  const { locale, t } = usePresentation();
  const [confirmAgain, setConfirmAgain] = useState(false);
  const count = (value: number) => value.toLocaleString(locale);

  if (preparation.state === "ready") {
    return (
      <section className="translation-card is-ready">
        <h3><CheckCircle2 aria-hidden="true" />{t("translation.readyTitle")}</h3>
        <p className="translation-card-summary">
          {preparation.preparedAt && preparation.preparedWith
            ? t("translation.readySummary", {
              count: count(preparation.total),
              date: new Date(preparation.preparedAt).toLocaleDateString(locale, {
                day: "numeric",
                month: "short",
                year: "numeric",
              }),
              model: preparation.preparedWith,
            })
            : t("translation.textsDone", { done: count(preparation.done), total: count(preparation.total) })}
        </p>
        {preparation.missing > 0 ? (
          <p className="library-manage-help">{t("translation.readyMissing", { count: count(preparation.missing) })}</p>
        ) : null}
        <p className="library-manage-help">{t("translation.readyHelp")}</p>
        <div className="library-manage-row">
          {onPlay ? (
            <button className="app-button is-primary" type="button" onClick={onPlay}>
              <Play aria-hidden="true" />{t("translation.play")}
            </button>
          ) : null}
          <button className="app-button" type="button" disabled={busy} onClick={onPrepare}>
            <RefreshCw aria-hidden="true" />{t("translation.checkNewLines")}
          </button>
          {confirmAgain ? null : (
            <button className="app-button" type="button" disabled={busy} onClick={() => setConfirmAgain(true)}>
              <Languages aria-hidden="true" />{`${t("translation.translateAgain")}…`}
            </button>
          )}
        </div>
        {confirmAgain ? (
          <div
            className="library-manage-confirm"
            role="alertdialog"
            aria-modal="false"
            aria-label={t("translation.translateAgainConfirm")}
          >
            <span>{t("translation.translateAgainConfirm")}</span>
            <button className="app-button is-primary" type="button" disabled={translatingAgain} onClick={onTranslateAgain}>
              {translatingAgain ? <LoaderCircle className="app-spin" aria-hidden="true" /> : null}
              {t("translation.translateAgain")}
            </button>
            <button className="library-manage-link" type="button" disabled={translatingAgain} onClick={() => setConfirmAgain(false)}>
              {t("common.cancel")}
            </button>
          </div>
        ) : null}
      </section>
    );
  }

  const percent = preparation.total > 0 ? Math.floor((preparation.done / preparation.total) * 100) : 0;
  const title = {
    preparing: "translation.preparing",
    waiting: "translation.waiting",
    paused: "translation.paused",
    failed: "translation.failed",
    needs_translator: "translation.needsTranslatorTitle",
  } as const;
  const barState = {
    preparing: "running",
    waiting: "waiting",
    paused: "paused",
    failed: "failed",
    needs_translator: "paused",
  } as const;
  const working = preparation.state === "preparing" || preparation.state === "waiting";
  return (
    <section className={`translation-card is-${preparation.state}`}>
      <div className="translation-card-head">
        <h3>
          {working ? <LoaderCircle className="app-spin" aria-hidden="true" /> : null}
          {t(title[preparation.state])}
        </h3>
        <strong>{`${percent}%`}</strong>
      </div>
      <ProgressBar percent={percent} state={barState[preparation.state]} label={t("translation.preparing")} />
      <div className="translation-card-counts">
        <span>{t("translation.textsDone", { done: count(preparation.done), total: count(preparation.total) })}</span>
        {preparation.state === "preparing" && preparation.secondsLeft !== null ? (
          <span>{timeLeftText(t, preparation.secondsLeft)}</span>
        ) : null}
      </div>
      <ul className="translation-groups">
        {preparation.groups.filter((group) => group.total > 0).map((group) => (
          <li key={group.group} className={group.done >= group.total ? "is-done" : undefined}>
            <span>{t(groupLabelKeys[group.group])}</span>
            <span>
              {t("translation.groupCount", { done: count(group.done), total: count(group.total) })}
              {group.done >= group.total ? <CheckCircle2 aria-hidden="true" /> : null}
            </span>
          </li>
        ))}
      </ul>
      {preparation.state === "failed" ? (
        <div className="translation-problem" role="alert">
          <p className="library-manage-result is-problem">
            <AlertTriangle aria-hidden="true" />{t(problemKeys[preparation.problem ?? "other"])}
          </p>
          {preparation.error ? (
            <p className="library-manage-help">{t("common.technicalDetail", { detail: preparation.error })}</p>
          ) : null}
        </div>
      ) : null}
      <div className="library-manage-row">
        {preparation.state === "preparing" || preparation.state === "waiting" ? (
          <button className="app-button" type="button" disabled={busy} onClick={onPause}>
            <Pause aria-hidden="true" />{t("translation.pause")}
          </button>
        ) : null}
        {preparation.state === "paused" ? (
          <button className="app-button is-primary" type="button" disabled={busy} onClick={onPrepare}>
            <Play aria-hidden="true" />{t("translation.resume")}
          </button>
        ) : null}
        {preparation.state === "failed" ? (
          <button className="app-button is-primary" type="button" disabled={busy} onClick={onPrepare}>
            <RefreshCw aria-hidden="true" />{t("translation.tryAgain")}
          </button>
        ) : null}
        {preparation.state === "needs_translator" && onOpenSettings ? (
          <button className="app-button is-primary" type="button" onClick={onOpenSettings}>
            {t("translation.openSettings")}
          </button>
        ) : null}
      </div>
    </section>
  );
}

function translatorLabel(
  settings: TranslationSettings | undefined,
  t: ReturnType<typeof usePresentation>["t"],
): string {
  const translator = settings?.translator;
  if (!translator) return t("translation.translatorNone");
  return translator.kind === "localModel"
    ? t("translation.translatorLocal", { model: translator.model })
    : t("translation.translatorDeepl");
}
