import { AlertTriangle, CheckCircle2, LoaderCircle, X } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";

import { usePresentation } from "../preferences/PresentationProvider";

export interface NoticeAction {
  label: string;
  onClick: () => void;
  primary?: boolean;
}

const NOTICE_MS = 10_000;

/**
 * A short confirmation at the bottom right, with up to two actions such as
 * Undo and Open. It closes by itself unless the pointer or focus is on it.
 */
export function Notice({
  tone = "success",
  title,
  detail,
  actions = [],
  onClose,
}: {
  tone?: "success" | "progress" | "problem";
  title: ReactNode;
  detail?: ReactNode;
  actions?: NoticeAction[];
  onClose: () => void;
}) {
  const { t } = usePresentation();
  const [held, setHeld] = useState(false);
  const close = useRef(onClose);
  close.current = onClose;

  useEffect(() => {
    if (held) return undefined;
    const timer = window.setTimeout(() => close.current(), NOTICE_MS);
    return () => window.clearTimeout(timer);
  }, [held, title]);

  const Icon = tone === "problem" ? AlertTriangle : tone === "progress" ? LoaderCircle : CheckCircle2;
  return createPortal(
    <div
      className={`app-notice is-${tone}`}
      role={tone === "problem" ? "alert" : "status"}
      onPointerEnter={() => setHeld(true)}
      onPointerLeave={() => setHeld(false)}
      onFocus={() => setHeld(true)}
      onBlur={() => setHeld(false)}
    >
      <Icon className="app-notice-icon" aria-hidden="true" />
      <div className="app-notice-copy">
        <strong>{title}</strong>
        {detail ? <span>{detail}</span> : null}
      </div>
      <button className="app-notice-close" type="button" aria-label={t("notice.close")} onClick={onClose}>
        <X aria-hidden="true" />
      </button>
      {actions.length ? (
        <div className="app-notice-actions">
          {actions.map((action) => (
            <button
              className={action.primary ? "is-primary" : undefined}
              type="button"
              key={action.label}
              onClick={action.onClick}
            >
              {action.label}
            </button>
          ))}
        </div>
      ) : null}
    </div>,
    noticeHost(),
  );
}

/** One stack for every notice, so two at once never cover each other. */
function noticeHost(): HTMLElement {
  const existing = document.getElementById("app-notices");
  if (existing) return existing;
  const host = document.createElement("div");
  host.id = "app-notices";
  host.className = "app-notices";
  document.body.append(host);
  return host;
}
