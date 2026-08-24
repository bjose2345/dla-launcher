import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Check,
  CircleCheck,
  FolderCog,
  FolderOpen,
  EyeOff,
  HardDrive,
  Heart,
  Keyboard,
  Languages,
  Library,
  LoaderCircle,
  Maximize2,
  Monitor,
  Palette,
  RotateCcw,
  TriangleAlert,
  X,
} from "lucide-react";
import { useEffect, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";

import type { WorkPreference, WorkPreferenceKind } from "../features/library/types";
import type { ScannerRootPreferenceGateway } from "../features/scanner/types";
import { scannerRootPreferenceKey } from "../features/scanner/types";
import { useKeyBindings } from "./KeyBindingsProvider";
import { formatByteSize } from "../formatByteSize";
import {
  coverCacheCapacities,
  coverCacheRetentions,
  type CoverCacheCapacity,
  type CoverCacheGateway,
  type CoverCacheRetention,
} from "./coverCache";
import {
  actionsInScope,
  bindingActions,
  bindingConflicts,
  bindingScopes,
  bindingsForAction,
  eventCombo,
  formatCombo,
  type BindingAction,
  type BindingScope,
} from "./keyBindings";
import { LocaleFlag } from "./LocaleFlag";
import { locales, themes } from "./preferences";
import { usePresentation } from "./PresentationProvider";
import { SettingsSection } from "./SettingsSection";
import { windowPresets, type WindowGateway, type WindowPreset } from "./windowSizing";

export const settingsTabs = ["general", "library", "display", "controls"] as const;

export type SettingsTab = (typeof settingsTabs)[number];

export function parseSettingsTab(value: unknown): SettingsTab {
  return settingsTabs.find((tab) => tab === value) ?? "general";
}

export interface SettingsRouteSearch {
  tab: SettingsTab;
  legacyAbout?: true;
}

export function parseSettingsSearch(search: Record<string, unknown>): SettingsRouteSearch {
  if (search.tab === "about") return { tab: "general", legacyAbout: true };
  return { tab: parseSettingsTab(search.tab) };
}

const tabLabelKeys = {
  general: "settings.tab.general",
  library: "settings.tab.library",
  display: "settings.tab.display",
  controls: "settings.tab.controls",
} as const;

const scopeLabelKeys = {
  global: "settings.scope.global",
  playback: "settings.scope.playback",
  video: "settings.scope.video",
  reader: "settings.scope.reader",
} as const;

export interface WorkPreferenceGateway {
  listWorkPreferences(): Promise<WorkPreference[]>;
  replaceWorkPreference(
    workCode: string,
    preference: WorkPreferenceKind | null,
  ): Promise<WorkPreference | null>;
}

export function SettingsPage({
  scannerRootGateway,
  windowGateway,
  tab = "general",
  onTabChange,
  coverCacheGateway,
  workPreferenceGateway,
  onOpenWork,
}: {
  scannerRootGateway?: ScannerRootPreferenceGateway;
  windowGateway?: WindowGateway;
  tab?: SettingsTab;
  onTabChange?: (tab: SettingsTab) => void;
  coverCacheGateway?: CoverCacheGateway;
  workPreferenceGateway?: WorkPreferenceGateway;
  onOpenWork?: (code: string) => void | Promise<void>;
}) {
  const { t } = usePresentation();
  const [localTab, setLocalTab] = useState<SettingsTab>(tab);
  const active = onTabChange ? tab : localTab;
  const select = (next: SettingsTab) => (onTabChange ? onTabChange(next) : setLocalTab(next));
  const moveTabFocus = (event: ReactKeyboardEvent<HTMLButtonElement>, current: SettingsTab) => {
    const index = settingsTabs.indexOf(current);
    let next: SettingsTab | undefined;
    if (event.key === "ArrowRight") next = settingsTabs[(index + 1) % settingsTabs.length];
    if (event.key === "ArrowLeft") next = settingsTabs[(index - 1 + settingsTabs.length) % settingsTabs.length];
    if (event.key === "Home") next = settingsTabs[0];
    if (event.key === "End") next = settingsTabs[settingsTabs.length - 1];
    if (!next) return;
    event.preventDefault();
    select(next);
    document.getElementById(`settings-tab-${next}`)?.focus();
  };

  return (
    <main className="settings-shell">
      <header className="settings-masthead">
        <span className="settings-eyebrow">{t("settings.eyebrow")}</span>
        <h1>{t("settings.title")}</h1>
        <div className="settings-tabs" role="tablist" aria-label={t("settings.title")}>
          {settingsTabs.map((value) => (
            <button
              className={value === active ? "active" : undefined}
              type="button"
              role="tab"
              id={`settings-tab-${value}`}
              aria-controls={`settings-panel-${value}`}
              aria-selected={value === active}
              tabIndex={value === active ? 0 : -1}
              key={value}
              onClick={() => select(value)}
              onKeyDown={(event) => moveTabFocus(event, value)}
            >
              <TabIcon tab={value} />
              {t(tabLabelKeys[value])}
            </button>
          ))}
        </div>
      </header>

      <div
        className="settings-body"
        role="tabpanel"
        id={`settings-panel-${active}`}
        aria-labelledby={`settings-tab-${active}`}
      >
        {active === "general" ? (
          <>
            <LanguageSection />
            {scannerRootGateway ? <ScannerRootSection gateway={scannerRootGateway} /> : null}
          </>
        ) : null}
        {active === "library" ? (
          <>
            <LibrarySection />
            {workPreferenceGateway ? (
              <WorkPreferenceSection gateway={workPreferenceGateway} onOpenWork={onOpenWork} />
            ) : null}
            {coverCacheGateway ? <CoverCacheSection gateway={coverCacheGateway} /> : null}
          </>
        ) : null}
        {active === "display" ? (
          <>
            <ThemeSection />
            {windowGateway ? <WindowSection gateway={windowGateway} /> : null}
          </>
        ) : null}
        {active === "controls" ? <KeyBindingSection /> : null}
      </div>
    </main>
  );
}

function TabIcon({ tab }: { tab: SettingsTab }) {
  if (tab === "general") return <Languages aria-hidden="true" />;
  if (tab === "library") return <Library aria-hidden="true" />;
  if (tab === "display") return <Monitor aria-hidden="true" />;
  return <Keyboard aria-hidden="true" />;
}

function LanguageSection() {
  const { locale, setLocale, t } = usePresentation();
  return (
    <SettingsSection
      icon={<Languages aria-hidden="true" />}
      title={t("settings.language")}
      description={t("settings.languageDescription")}
    >
      <div className="settings-options">
        {locales.map((option) => (
          <button
            className={`settings-option${option.id === locale ? " selected" : ""}`}
            type="button"
            aria-pressed={option.id === locale}
            onClick={() => setLocale(option.id)}
            key={option.id}
          >
            <LocaleFlag locale={option.id} />
            <span><strong>{option.label}</strong><small>{option.shortLabel}</small></span>
            <Check className="settings-option-check" aria-hidden="true" />
          </button>
        ))}
      </div>
    </SettingsSection>
  );
}

function ThemeSection() {
  const { setTheme, t, theme } = usePresentation();
  return (
    <SettingsSection
      icon={<Palette aria-hidden="true" />}
      title={t("settings.theme")}
      description={t("settings.themeDescription")}
    >
      <div className="settings-options">
        {themes.map((option) => (
          <button
            className={`settings-option${option.id === theme ? " selected" : ""}`}
            type="button"
            aria-pressed={option.id === theme}
            onClick={() => setTheme(option.id)}
            key={option.id}
          >
            <span className="settings-swatches" aria-hidden="true">
              {option.colors.map((color) => <i style={{ backgroundColor: color }} key={color} />)}
            </span>
            <span><strong>{t(option.labelKey)}</strong></span>
            <Check className="settings-option-check" aria-hidden="true" />
          </button>
        ))}
      </div>
    </SettingsSection>
  );
}

function LibrarySection() {
  const { includeUnreviewed, setIncludeUnreviewed, setShowPlayTime, showPlayTime, t } = usePresentation();
  return (
    <SettingsSection
      icon={<Library aria-hidden="true" />}
      title={t("settings.libraryDisplay")}
      description={t("settings.libraryDisplayDescription")}
    >
      <div className="settings-switches">
        <SettingsSwitch
          checked={showPlayTime}
          label={t("settings.showPlayTime")}
          help={t("settings.showPlayTimeHelp")}
          onChange={setShowPlayTime}
        />
        <SettingsSwitch
          checked={includeUnreviewed}
          label={t("settings.includeUnreviewed")}
          help={t("settings.includeUnreviewedHelp")}
          onChange={setIncludeUnreviewed}
        />
      </div>
    </SettingsSection>
  );
}

function WorkPreferenceSection({
  gateway,
  onOpenWork,
}: {
  gateway: WorkPreferenceGateway;
  onOpenWork?: (code: string) => void | Promise<void>;
}) {
  const { t } = usePresentation();
  const queryClient = useQueryClient();
  const preferences = useQuery({
    queryKey: ["library", "preferences"],
    queryFn: () => gateway.listWorkPreferences(),
  });
  const change = useMutation({
    mutationFn: ({ workCode, preference }: {
      workCode: string;
      preference: WorkPreferenceKind | null;
    }) => gateway.replaceWorkPreference(workCode, preference),
    onSettled: async (_result, _error, variables) => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["library", "preferences"] }),
        queryClient.invalidateQueries({ queryKey: ["library", "personalization"] }),
        queryClient.invalidateQueries({ queryKey: ["library", "preference", variables.workCode] }),
      ]);
    },
  });
  const changingCode = change.isPending ? change.variables?.workCode ?? null : null;
  const ordered = [...(preferences.data ?? [])].sort((left, right) => (
    left.preference.localeCompare(right.preference)
    || right.updatedAt.localeCompare(left.updatedAt)
    || left.workCode.localeCompare(right.workCode)
  ));
  const error = preferences.error ?? change.error;

  return (
    <SettingsSection
      icon={<Heart aria-hidden="true" />}
      title={t("library.preferences")}
      description={t("library.preferencesHelp")}
    >
      {error ? (
        <p className="settings-error" role="alert">
          {t("common.requestFailed", { error: String(error) })}
        </p>
      ) : null}
      {preferences.isPending ? (
        <p className="settings-note">
          <LoaderCircle className="settings-spin" aria-hidden="true" />{t("library.preferencesLoading")}
        </p>
      ) : ordered.length === 0 ? (
        <p className="settings-note">{t("library.preferencesEmpty")}</p>
      ) : (
        <ul className="settings-preference-list">
          {ordered.map((item) => (
            <li key={item.workCode}>
              <div>
                {onOpenWork ? (
                  <button
                    className="settings-preference-work"
                    type="button"
                    onClick={() => void onOpenWork(item.workCode)}
                  >
                    {item.workCode}
                  </button>
                ) : <strong>{item.workCode}</strong>}
                <small>{t(item.preference === "favorite" ? "library.favorite" : "library.notInterested")}</small>
              </div>
              <div className="settings-preference-actions">
                <button
                  aria-pressed={item.preference === "favorite"}
                  disabled={changingCode === item.workCode}
                  title={t("library.favorite")}
                  aria-label={t("library.favorite")}
                  type="button"
                  onClick={() => change.mutate({ workCode: item.workCode, preference: "favorite" })}
                >
                  <Heart aria-hidden="true" />
                </button>
                <button
                  aria-pressed={item.preference === "not_interested"}
                  disabled={changingCode === item.workCode}
                  title={t("library.notInterested")}
                  aria-label={t("library.notInterested")}
                  type="button"
                  onClick={() => change.mutate({ workCode: item.workCode, preference: "not_interested" })}
                >
                  <EyeOff aria-hidden="true" />
                </button>
                <button
                  disabled={changingCode === item.workCode}
                  title={t("library.removePreference")}
                  aria-label={t("library.removePreference")}
                  type="button"
                  onClick={() => change.mutate({ workCode: item.workCode, preference: null })}
                >
                  {changingCode === item.workCode
                    ? <LoaderCircle className="settings-spin" aria-hidden="true" />
                    : <X aria-hidden="true" />}
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}
    </SettingsSection>
  );
}

const retentionLabelKeys = {
  days_90: "settings.coverRetention90",
  days_180: "settings.coverRetention180",
  days_360: "settings.coverRetention360",
  never: "settings.coverRetentionNever",
} as const;

const capacityLabelKeys = {
  standard: "settings.coverCapacityStandard",
  large: "settings.coverCapacityLarge",
  very_large: "settings.coverCapacityVeryLarge",
  unlimited: "settings.coverCapacityUnlimited",
} as const;

function CoverCacheSection({ gateway }: { gateway: CoverCacheGateway }) {
  const { locale, t } = usePresentation();
  const queryClient = useQueryClient();
  const summary = useQuery({
    queryKey: ["settings", "cover-cache"],
    queryFn: () => gateway.readSummary(),
  });
  const configure = useMutation({
    mutationFn: ({ retention, capacity }: {
      retention: CoverCacheRetention;
      capacity: CoverCacheCapacity;
    }) => gateway.configure(retention, capacity),
    onSuccess: (value) => queryClient.setQueryData(["settings", "cover-cache"], value),
  });
  const data = summary.data;
  const pending = summary.isPending || configure.isPending;
  const updateRetention = (retention: CoverCacheRetention) => {
    if (data) configure.mutate({ retention, capacity: data.capacity });
  };
  const updateCapacity = (capacity: CoverCacheCapacity) => {
    if (data) configure.mutate({ retention: data.retention, capacity });
  };
  const error = summary.error ?? configure.error;

  return (
    <SettingsSection
      icon={<HardDrive aria-hidden="true" />}
      title={t("settings.coverCache")}
      description={t("settings.coverCacheDescription")}
    >
      <div className="settings-cache-controls">
        <div>
          <h3>{t("settings.coverRetention")}</h3>
          <p>{t("settings.coverRetentionDescription")}</p>
          <div className="settings-options">
            {coverCacheRetentions.map((retention) => (
              <button
                className={`settings-option settings-cache-option${data?.retention === retention ? " selected" : ""}`}
                type="button"
                disabled={pending}
                aria-pressed={data?.retention === retention}
                onClick={() => updateRetention(retention)}
                key={retention}
              >
                <span><strong>{t(retentionLabelKeys[retention])}</strong></span>
                <Check className="settings-option-check" aria-hidden="true" />
              </button>
            ))}
          </div>
        </div>
        <div>
          <h3>{t("settings.coverCapacity")}</h3>
          <p>{t("settings.coverCapacityDescription")}</p>
          <div className="settings-options">
            {coverCacheCapacities.map((capacity) => {
              const limits = coverCapacityLimits(capacity);
              return (
                <button
                  className={`settings-option settings-cache-option${data?.capacity === capacity ? " selected" : ""}`}
                  type="button"
                  disabled={pending}
                  aria-pressed={data?.capacity === capacity}
                  onClick={() => updateCapacity(capacity)}
                  key={capacity}
                >
                  <span>
                    <strong>{t(capacityLabelKeys[capacity])}</strong>
                    <small>
                      {limits
                        ? t("settings.coverCapacityImages", { count: limits.entries.toLocaleString(locale) })
                        : t("settings.coverCapacityUnlimitedHelp")}
                    </small>
                  </span>
                  <Check className="settings-option-check" aria-hidden="true" />
                </button>
              );
            })}
          </div>
        </div>
      </div>
      {data ? (
        <p className="settings-note settings-cache-status">
          {t("settings.coverCacheStored", {
            count: data.entryCount.toLocaleString(locale),
            size: formatByteSize(data.storedBytes, locale),
          })}
          <span>{t("settings.coverCachePolicyHelp")}</span>
        </p>
      ) : null}
      {summary.isPending ? (
        <p className="settings-note settings-loading">
          <LoaderCircle className="settings-spin" aria-hidden="true" />
          {t("settings.coverCacheLoading")}
        </p>
      ) : null}
      {error ? (
        <p className="settings-error" role="alert">
          {t("common.requestFailed", { error: String(error) })}
        </p>
      ) : null}
    </SettingsSection>
  );
}

function coverCapacityLimits(capacity: CoverCacheCapacity): { entries: number } | null {
  if (capacity === "standard") return { entries: 4_000 };
  if (capacity === "large") return { entries: 16_000 };
  if (capacity === "very_large") return { entries: 64_000 };
  return null;
}

function WindowSection({ gateway }: { gateway: WindowGateway }) {
  const { t } = usePresentation();
  const queryClient = useQueryClient();
  const metrics = useQuery({
    queryKey: ["settings", "window-metrics"],
    queryFn: () => gateway.readWindowMetrics(),
  });
  const resize = useMutation({
    mutationFn: (preset: WindowPreset | null) => (
      preset ? gateway.resizeWindow(preset) : gateway.maximizeWindow()
    ),
    onSuccess: (value) => queryClient.setQueryData(["settings", "window-metrics"], value),
  });
  const data = metrics.data;
  const supported = data?.supportsWindowControls ?? false;

  return (
    <SettingsSection
      icon={<Monitor aria-hidden="true" />}
      title={t("settings.windowSize")}
      description={t("settings.windowSizeDescription")}
      action={supported ? (
        <button className="settings-button" type="button" disabled={resize.isPending} onClick={() => resize.mutate(null)}>
          <Maximize2 aria-hidden="true" />{t("settings.maximize")}
        </button>
      ) : undefined}
    >
      {data && !supported ? <p className="settings-note">{t("settings.windowUnsupported")}</p> : null}
      {supported ? (
        <div className="settings-presets">
          {windowPresets.map((preset) => {
            const current = data
              ? Math.abs(data.width - preset.width) < 4 && Math.abs(data.height - preset.height) < 4
              : false;
            const tooLarge = data
              ? preset.width > data.workAreaWidth || preset.height > data.workAreaHeight
              : false;
            return (
              <button
                className={`settings-preset${current ? " current" : ""}${tooLarge ? " is-too-large" : ""}`}
                type="button"
                disabled={tooLarge || resize.isPending}
                aria-pressed={current}
                key={preset.id}
                onClick={() => resize.mutate(preset)}
              >
                <strong>{preset.label}</strong>
                <span>
                  {current
                    ? t("settings.windowCurrent")
                    : tooLarge
                      ? t("settings.windowTooLarge")
                      : t("settings.windowPreset")}
                </span>
              </button>
            );
          })}
        </div>
      ) : null}
      {data ? (
        <div className="settings-metrics">
          <span><small>{t("settings.windowMetricWindow")}</small><b>{data.width} × {data.height}</b></span>
          <span><small>{t("settings.windowMetricArea")}</small><b>{data.workAreaWidth} × {data.workAreaHeight}</b></span>
          <span><small>{t("settings.windowMetricScale")}</small><b>{data.scaleFactor.toFixed(2)}×</b></span>
          <span><small>{t("settings.windowMetricState")}</small><b>{t(data.maximized ? "settings.windowMaximized" : "settings.windowWindowed")}</b></span>
        </div>
      ) : null}
      {metrics.isPending ? (
        <p className="settings-note settings-loading">
          <LoaderCircle className="settings-spin" aria-hidden="true" />
          {t("settings.windowLoading")}
        </p>
      ) : null}
      {metrics.error ? (
        <p className="settings-error" role="alert">
          {t("common.requestFailed", { error: String(metrics.error) })}
        </p>
      ) : null}
      {resize.error ? (
        <p className="settings-error" role="alert">{t("common.requestFailed", { error: String(resize.error) })}</p>
      ) : null}
    </SettingsSection>
  );
}

function KeyBindingSection() {
  const { t } = usePresentation();
  const { assign, bindings, reset, resetAll } = useKeyBindings();
  const [listening, setListening] = useState<{ actionId: string; slot: number } | null>(null);
  const [feedback, setFeedback] = useState<BindingFeedback | null>(null);

  useEffect(() => {
    if (!listening) return;
    const onKeyDown = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopImmediatePropagation();
      if (event.key === "Escape") {
        setListening(null);
        return;
      }
      const combo = eventCombo(event);
      if (!combo) return;
      const conflicts = bindingConflicts(bindings, listening.actionId, combo);
      const fixed = conflicts.filter((conflict) => conflict.fixed);
      if (fixed.length) {
        setFeedback({
          actionId: listening.actionId,
          others: fixed.map((conflict) => conflict.id),
          kind: "reserved",
        });
        setListening(null);
        return;
      }
      assign(listening.actionId, listening.slot, combo);
      setFeedback(conflicts.length ? {
        actionId: listening.actionId,
        others: conflicts.map((conflict) => conflict.id),
        kind: "reassigned",
      } : null);
      setListening(null);
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [assign, bindings, listening]);

  return (
    <SettingsSection
      icon={<Keyboard aria-hidden="true" />}
      title={t("settings.keyBindings")}
      description={t("settings.keyBindingsDescription")}
      action={(
        <button className="settings-button" type="button" onClick={() => { resetAll(); setFeedback(null); setListening(null); }}>
          <RotateCcw aria-hidden="true" />{t("settings.bindingResetAll")}
        </button>
      )}
    >
      <div className="settings-bindings">
        {bindingScopes.map((scope) => (
          <BindingScopeGroup
            scope={scope}
            listening={listening}
            feedback={feedback}
            onListen={(actionId, slot) => { setFeedback(null); setListening({ actionId, slot }); }}
            onReset={(actionId) => { reset(actionId); setFeedback(null); setListening(null); }}
            key={scope}
          />
        ))}
      </div>
      <p className="settings-note">{t("settings.bindingStorage")}</p>
    </SettingsSection>
  );
}

function BindingScopeGroup({
  scope,
  listening,
  feedback,
  onListen,
  onReset,
}: {
  scope: BindingScope;
  listening: { actionId: string; slot: number } | null;
  feedback: BindingFeedback | null;
  onListen: (actionId: string, slot: number) => void;
  onReset: (actionId: string) => void;
}) {
  const { t } = usePresentation();
  const { bindings } = useKeyBindings();
  const actions = actionsInScope(scope);
  if (!actions.length) return null;

  return (
    <>
      <p className="settings-binding-scope">{t(scopeLabelKeys[scope])}</p>
      {actions.map((action) => (
        <BindingRow
          action={action}
          combos={bindingsForAction(bindings, action.id)}
          listening={listening?.actionId === action.id ? listening.slot : null}
          feedback={feedback?.actionId === action.id ? feedback : null}
          onListen={onListen}
          onReset={onReset}
          key={action.id}
        />
      ))}
    </>
  );
}

function BindingRow({
  action,
  combos,
  listening,
  feedback,
  onListen,
  onReset,
}: {
  action: BindingAction;
  combos: readonly string[];
  listening: number | null;
  feedback: BindingFeedback | null;
  onListen: (actionId: string, slot: number) => void;
  onReset: (actionId: string) => void;
}) {
  const { locale, t } = usePresentation();
  const feedbackActions = feedback?.others.flatMap((actionId) => {
    const found = bindingActions.find((candidate) => candidate.id === actionId);
    return found ? [found] : [];
  }) ?? [];
  const feedbackNames = new Intl.ListFormat(locale, { style: "long", type: "conjunction" })
    .format(feedbackActions.map((candidate) => t(candidate.labelKey)));
  const slots = Math.min(action.slots, combos.length + 1);

  return (
    <div className={`settings-binding${feedback?.kind === "reserved" ? " is-conflict" : ""}${feedback?.kind === "reassigned" ? " has-reassignment" : ""}${listening !== null ? " is-listening" : ""}`}>
      <span className="settings-binding-name">
        {t(action.labelKey)}
        {action.helpKey ? <em>{t(action.helpKey)}</em> : null}
        {action.fixed ? <em>{t("settings.bindingFixed")}</em> : null}
      </span>
      <span className="settings-binding-keys">
        {Array.from({ length: action.fixed ? combos.length : slots }, (_, slot) => {
          const combo = combos[slot];
          const isListening = listening === slot;
          return (
            <button
              className={`${combo ? "" : "empty"}${isListening ? " listening" : ""}`.trim() || undefined}
              type="button"
              disabled={action.fixed}
              key={slot}
              onClick={() => onListen(action.id, slot)}
            >
              {isListening
                ? t("settings.bindingListening")
                : combo
                  ? formatCombo(combo)
                  : t("settings.bindingAdd")}
            </button>
          );
        })}
      </span>
      {action.fixed ? <span /> : (
        <button
          className="settings-binding-reset"
          type="button"
          aria-label={t("settings.bindingReset")}
          title={t("settings.bindingReset")}
          onClick={() => onReset(action.id)}
        >
          <RotateCcw aria-hidden="true" />
        </button>
      )}
      {feedbackActions.length ? (
        <p
          className={`settings-binding-warning is-${feedback?.kind}`}
          role={feedback?.kind === "reserved" ? "alert" : "status"}
        >
          {feedback?.kind === "reserved"
            ? <TriangleAlert aria-hidden="true" />
            : <CircleCheck aria-hidden="true" />}
          {t(
            feedback?.kind === "reserved"
              ? "settings.bindingReserved"
              : "settings.bindingReassigned",
            { action: feedbackNames },
          )}
        </p>
      ) : null}
    </div>
  );
}

interface BindingFeedback {
  actionId: string;
  others: string[];
  kind: "reassigned" | "reserved";
}

function ScannerRootSection({ gateway }: { gateway: ScannerRootPreferenceGateway }) {
  const { t } = usePresentation();
  const queryClient = useQueryClient();
  const preference = useQuery({
    queryKey: scannerRootPreferenceKey,
    queryFn: () => gateway.readRootPreference(),
  });
  const choose = useMutation({
    mutationFn: () => gateway.selectPreferredRoot(),
    onSuccess: (result) => {
      if (result) queryClient.setQueryData(scannerRootPreferenceKey, result);
    },
  });
  const reset = useMutation({
    mutationFn: () => gateway.resetPreferredRoot(),
    onSuccess: (result) => queryClient.setQueryData(scannerRootPreferenceKey, result),
  });
  const error = preference.error ?? choose.error ?? reset.error;
  const pending = choose.isPending || reset.isPending;
  const source = preference.data?.source === "configured"
    ? t("settings.scannerRootConfigured")
    : t("settings.scannerRootDefault");

  return (
    <SettingsSection
      icon={<FolderCog aria-hidden="true" />}
      title={t("settings.scannerRoot")}
      description={t("settings.scannerRootDescription")}
    >
      <div className="settings-folder">
        <div>
          <small>{source}</small>
          {preference.isPending ? (
            <strong><LoaderCircle className="settings-spin" aria-hidden="true" />{t("settings.scannerRootLoading")}</strong>
          ) : (
            <strong>{preference.data?.displayPath ?? t("settings.scannerRootUnavailable")}</strong>
          )}
          {preference.data ? (
            <span className={`settings-folder-state${preference.data.available ? "" : " pending"}`}>
              {preference.data.available ? <CircleCheck aria-hidden="true" /> : <TriangleAlert aria-hidden="true" />}
              {preference.data.available
                ? t("settings.scannerRootReady")
                : preference.data.canPrepare
                  ? t("settings.scannerRootCreatedOnScan")
                  : t("settings.scannerRootUnavailable")}
            </span>
          ) : null}
        </div>
        <div className="settings-folder-actions">
          <button className="settings-button" type="button" disabled={pending} onClick={() => choose.mutate()}>
            {choose.isPending ? <LoaderCircle className="settings-spin" aria-hidden="true" /> : <FolderOpen aria-hidden="true" />}
            {t("settings.chooseScannerRoot")}
          </button>
          <button className="settings-button is-ghost" type="button" disabled={pending} onClick={() => reset.mutate()}>
            {reset.isPending ? <LoaderCircle className="settings-spin" aria-hidden="true" /> : <RotateCcw aria-hidden="true" />}
            {t("settings.resetScannerRoot")}
          </button>
        </div>
      </div>
      {error ? <p className="settings-error" role="alert">{t("common.requestFailed", { error: String(error) })}</p> : null}
    </SettingsSection>
  );
}

function SettingsSwitch({
  checked,
  label,
  help,
  onChange,
}: {
  checked: boolean;
  label: string;
  help: string;
  onChange: (checked: boolean) => void;
}) {
  return (
    <button
      className="settings-switch"
      type="button"
      role="switch"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
    >
      <span>
        <strong>{label}</strong>
        <small>{help}</small>
      </span>
      <span className="settings-switch-track" aria-hidden="true" />
    </button>
  );
}
