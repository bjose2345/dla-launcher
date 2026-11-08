import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, Check, CheckCircle2, Languages, LoaderCircle } from "lucide-react";
import { useEffect, useId, useState, type ReactNode } from "react";

import { Switch } from "../../app/Switch";
import { usePresentation } from "../../preferences/PresentationProvider";
import { SettingsSection } from "../../preferences/SettingsSection";
import { translationSettingsKey } from "./GameTranslationTab";
import { TranslatorHelpDialog } from "./TranslatorHelpDialog";
import {
  localModelDefaults,
  type TranslationGateway,
  type TranslationSettings,
  type TranslatorSettings,
} from "./types";

type Kind = TranslatorSettings["kind"];

/**
 * Settings › Translation: the player's own translator. Each change saves as
 * it is made; Test translator tries what the fields hold.
 */
export function TranslationSettingsSection({ gateway }: { gateway: TranslationGateway }) {
  const { t } = usePresentation();
  const queryClient = useQueryClient();
  const settings = useQuery({ queryKey: translationSettingsKey, queryFn: () => gateway.readSettings() });
  const [kind, setKind] = useState<Kind>("localModel");
  const [address, setAddress] = useState<string>(localModelDefaults.address);
  const [model, setModel] = useState<string>(localModelDefaults.model);
  const [freePlan, setFreePlan] = useState(true);
  const [deeplKey, setDeeplKey] = useState("");
  const [help, setHelp] = useState(false);

  // The saved settings fill the fields once they arrive.
  useEffect(() => {
    const translator = settings.data?.translator;
    if (!translator) return;
    setKind(translator.kind);
    if (translator.kind === "localModel") {
      setAddress(translator.address);
      setModel(translator.model);
    } else {
      setFreePlan(translator.freePlan);
    }
  }, [settings.data]);

  const translator = (next: Partial<{ kind: Kind; address: string; model: string; freePlan: boolean }> = {}) => {
    const value = { kind, address, model, freePlan, ...next };
    return value.kind === "localModel"
      ? { kind: "localModel" as const, address: value.address.trim(), model: value.model.trim() }
      : { kind: "deepl" as const, freePlan: value.freePlan };
  };
  const save = useMutation({
    mutationFn: ({ next, key }: { next: TranslatorSettings; key?: string }) =>
      gateway.saveSettings({ translator: next, deeplKey: key }),
    onSuccess: (saved: TranslationSettings) => {
      queryClient.setQueryData(translationSettingsKey, saved);
      void queryClient.invalidateQueries({ queryKey: ["translation", "game"] });
    },
  });
  // A translator that works is the one the player wants: it is kept.
  const test = useMutation({
    mutationFn: () => gateway.testTranslator(translator(), deeplKey.trim() || undefined),
    onSuccess: () => save.mutate({ next: translator(), key: deeplKey.trim() || undefined }),
  });
  const saveChange = (next: Partial<{ kind: Kind; address: string; model: string; freePlan: boolean }>) => {
    test.reset();
    save.mutate({ next: translator(next) });
  };

  return (
    <SettingsSection
      icon={<Languages aria-hidden="true" />}
      title={t("settings.translation")}
      description={t("settings.translationDescription")}
    >
      <fieldset className="translation-settings-choice">
        <legend>{t("settings.translator")}</legend>
        <div className="settings-options">
          {(["localModel", "deepl"] as const).map((value) => (
            <button
              className={`settings-option${kind === value ? " selected" : ""}`}
              type="button"
              aria-pressed={kind === value}
              disabled={settings.isPending}
              key={value}
              onClick={() => {
                setKind(value);
                saveChange({ kind: value });
              }}
            >
              <span>
                <strong>{t(value === "localModel" ? "settings.translatorLocal" : "settings.translatorDeepl")}</strong>
                <small className="translation-settings-option-help">
                  {t(value === "localModel" ? "settings.translatorLocalHelp" : "settings.translatorDeeplHelp")}
                </small>
              </span>
              <Check className="settings-option-check" aria-hidden="true" />
            </button>
          ))}
        </div>
      </fieldset>

      {kind === "localModel" ? (
        <div className="translation-settings-fields">
          <Field label={t("settings.translatorAddress")} help={t("settings.translatorAddressHelp")}>
            {(props) => (
              <input
                {...props}
                className="library-manage-input"
                type="url"
                value={address}
                spellCheck={false}
                onChange={(event) => setAddress(event.target.value)}
                onBlur={() => saveChange({})}
              />
            )}
          </Field>
          <Field label={t("settings.translatorModel")} help={t("settings.translatorModelHelp")}>
            {(props) => (
              <input
                {...props}
                className="library-manage-input"
                type="text"
                value={model}
                spellCheck={false}
                onChange={(event) => setModel(event.target.value)}
                onBlur={() => saveChange({})}
              />
            )}
          </Field>
        </div>
      ) : (
        <div className="translation-settings-fields">
          <Field
            label={t("settings.deeplKey")}
            help={t(settings.data?.deeplKeySaved ? "settings.deeplKeySaved" : "settings.deeplKeyHelp")}
          >
            {(props) => (
              <input
                {...props}
                className="library-manage-input"
                type="password"
                value={deeplKey}
                autoComplete="off"
                placeholder={settings.data?.deeplKeySaved ? "••••••••" : undefined}
                onChange={(event) => setDeeplKey(event.target.value)}
                onBlur={() => {
                  if (deeplKey.trim()) {
                    test.reset();
                    save.mutate({ next: translator(), key: deeplKey.trim() });
                  }
                }}
              />
            )}
          </Field>
          <Switch
            checked={freePlan}
            label={t("settings.deeplFreePlan")}
            help={t("settings.deeplFreePlanHelp")}
            onChange={(next) => {
              setFreePlan(next);
              saveChange({ freePlan: next });
            }}
          />
        </div>
      )}

      <div className="translation-settings-test">
        <button className="app-button is-quiet" type="button" disabled={test.isPending} onClick={() => test.mutate()}>
          {test.isPending ? <LoaderCircle className="app-spin" aria-hidden="true" /> : null}
          {t(test.isPending ? "settings.translatorTesting" : "settings.translatorTest")}
        </button>
        {test.data ? (
          <p className="translation-settings-result is-ok" role="status">
            <CheckCircle2 aria-hidden="true" />
            <span>
              {t("settings.translatorWorks", {
                source: test.data.source,
                english: test.data.english,
                seconds: test.data.seconds.toFixed(1),
              })}
            </span>
          </p>
        ) : null}
        {test.error ? (
          <p className="translation-settings-result is-problem" role="alert">
            <AlertTriangle aria-hidden="true" />
            <span>{t("settings.translatorFailed", { error: String(test.error) })}</span>
          </p>
        ) : null}
      </div>

      <p className="settings-note translation-settings-foot">
        <button className="translation-help-link" type="button" onClick={() => setHelp(true)}>
          {t("translation.howToSetUp")}
        </button>
        <span>{t("settings.translationEngines")}</span>
      </p>
      {settings.error ?? save.error ? (
        <p className="app-callout" role="alert">
          {t("common.requestFailed", { error: String(settings.error ?? save.error) })}
        </p>
      ) : null}
      {help ? <TranslatorHelpDialog gateway={gateway} onClose={() => setHelp(false)} /> : null}
    </SettingsSection>
  );
}

/** A labelled text field with its help line, named by the label alone. */
function Field({
  label,
  help,
  children,
}: {
  label: string;
  help: string;
  children: (props: { id: string; "aria-describedby": string }) => ReactNode;
}) {
  const id = useId();
  const helpId = useId();
  return (
    <div className="translation-settings-field">
      <label htmlFor={id}>{label}</label>
      {children({ id, "aria-describedby": helpId })}
      <small id={helpId}>{help}</small>
    </div>
  );
}
