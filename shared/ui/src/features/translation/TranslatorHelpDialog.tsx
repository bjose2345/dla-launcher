import { Check, Copy, X } from "lucide-react";
import { useId, useRef, useState, type ReactNode } from "react";

import { useCopyFeedback } from "../../app/useCopyFeedback";
import { useModalDialog } from "../../app/useModalDialog";
import { usePresentation } from "../../preferences/PresentationProvider";
import type { TranslationGateway } from "./types";

type Program = "ollama" | "lmStudio";

const OLLAMA_SITE = "https://ollama.com/";
const LM_STUDIO_SITE = "https://lmstudio.ai/";
const OLLAMA_PULL = "ollama pull qwen3:4b-instruct";
const LM_STUDIO_MODEL = "qwen/qwen3-4b-2507";
const LM_STUDIO_ADDRESS = "http://127.0.0.1:1234/v1";

/** How to set up a free local translator, with Ollama or LM Studio. */
export function TranslatorHelpDialog({
  gateway,
  onClose,
}: {
  gateway: Pick<TranslationGateway, "openWebsite">;
  onClose: () => void;
}) {
  const { t } = usePresentation();
  const headingId = useId();
  const panel = useRef<HTMLDivElement>(null);
  const [program, setProgram] = useState<Program>("ollama");

  useModalDialog({ dialog: panel, onClose });

  const site = (url: string) => (
    <button className="translation-help-link" type="button" onClick={() => void gateway.openWebsite(url)}>
      {url.replace("https://", "").replace(/\/$/, "")}
    </button>
  );

  return (
    <div className="translation-help-backdrop" onClick={onClose}>
      <div
        ref={panel}
        className="translation-help"
        role="dialog"
        aria-modal="true"
        aria-labelledby={headingId}
        tabIndex={-1}
        onClick={(event) => event.stopPropagation()}
      >
        <header className="translation-help-head">
          <h2 id={headingId}>{t("translation.help.title")}</h2>
          <button className="library-manage-close" type="button" aria-label={t("translation.help.close")} onClick={onClose}>
            <X aria-hidden="true" />
          </button>
        </header>
        <div className="translation-help-body">
          <p>{t("translation.help.lead")}</p>
          <div className="translation-help-tabs" role="tablist" aria-label={t("translation.help.program")}>
            {(["ollama", "lmStudio"] as const).map((value) => (
              <button
                type="button"
                role="tab"
                aria-selected={program === value}
                className={program === value ? "active" : undefined}
                key={value}
                onClick={() => setProgram(value)}
              >
                {value === "ollama" ? "Ollama" : "LM Studio"}
              </button>
            ))}
          </div>
          {program === "ollama" ? (
            <ol className="translation-help-steps">
              <Step title={t("translation.help.ollama.install")}>
                {withPart(t("translation.help.installHelp", { site: PART }), site(OLLAMA_SITE))}
              </Step>
              <Step title={t("translation.help.model")}>
                {t("translation.help.ollama.modelHelp")}
                <CopyLine text={OLLAMA_PULL} />
              </Step>
              <Step title={t("translation.help.test")}>{t("translation.help.ollama.testHelp")}</Step>
            </ol>
          ) : (
            <ol className="translation-help-steps">
              <Step title={t("translation.help.lmStudio.install")}>
                {withPart(t("translation.help.installHelp", { site: PART }), site(LM_STUDIO_SITE))}
              </Step>
              <Step title={t("translation.help.model")}>{t("translation.help.lmStudio.modelHelp")}</Step>
              <Step title={t("translation.help.lmStudio.server")}>{t("translation.help.lmStudio.serverHelp")}</Step>
              <Step title={t("translation.help.test")}>
                {t("translation.help.lmStudio.testHelp")}
                <CopyLine text={LM_STUDIO_ADDRESS} />
                <CopyLine text={LM_STUDIO_MODEL} />
              </Step>
            </ol>
          )}
          <dl className="translation-help-facts">
            <dt>{t("translation.help.expect")}</dt>
            <dd>{t("translation.help.expectHelp")}</dd>
            <dt>{t("translation.help.quality")}</dt>
            <dd>{t("translation.help.qualityHelp")}</dd>
          </dl>
          <p className="library-manage-help">
            <strong>{t("translation.help.llamaCpp")}</strong>{" "}
            {withPart(t("translation.help.llamaCppHelp", { flag: PART }), <code>--cache-ram 0</code>)}
          </p>
        </div>
        <footer className="translation-help-foot">
          <button className="app-button is-primary" type="button" onClick={onClose}>
            {t("translation.help.done")}
          </button>
        </footer>
      </div>
    </div>
  );
}

// A message with an element inside it (a link, a code sample): the element
// takes the place of its placeholder, wherever a language puts it.
const PART = "\u0000";

function withPart(message: string, part: ReactNode): ReactNode {
  const [before = "", after = ""] = message.split(PART);
  return <>{before}{part}{after}</>;
}

function Step({ title, children }: { title: string; children: ReactNode }) {
  return (
    <li>
      <strong>{title}</strong>
      <span>{children}</span>
    </li>
  );
}

function CopyLine({ text }: { text: string }) {
  const { t } = usePresentation();
  const { result, copy } = useCopyFeedback();
  const copied = result === "copied";
  return (
    <span className="translation-help-copy">
      <code>{text}</code>
      <button className="app-button" type="button" onClick={() => copy(text)}>
        {copied ? <Check aria-hidden="true" /> : <Copy aria-hidden="true" />}
        {t(copied ? "translation.help.copied" : "translation.help.copy")}
      </button>
    </span>
  );
}
