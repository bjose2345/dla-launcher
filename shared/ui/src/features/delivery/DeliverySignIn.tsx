import { Archive, ArrowRight, CircleAlert, Eye, EyeOff, KeyRound, LoaderCircle, UserRound, X } from "lucide-react";
import { useEffect, useRef, useState, type FormEvent } from "react";
import { createPortal } from "react-dom";

import { usePresentation } from "../../preferences";
import { patronPlansUrl, problemText } from "./deliveryText";
import { deliveryFailure, type DeliveryGateway, type DeliveryProblem, type DeliveryView } from "./types";

/**
 * Signs in to the archive with the subscriber's FTP (SFTPGo) account. The
 * password goes to the launcher's Rust side and is kept there only while it
 * runs.
 */
export function DeliverySignIn({
  gateway,
  onSignedIn,
  onClose,
  onOpenExternal,
}: {
  gateway: DeliveryGateway;
  onSignedIn: (view: DeliveryView) => void;
  onClose: () => void;
  onOpenExternal?: (url: string) => Promise<void>;
}) {
  const { locale, t } = usePresentation();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [pending, setPending] = useState(false);
  const [showPassword, setShowPassword] = useState(false);
  const [problem, setProblem] = useState<DeliveryProblem | null>(null);
  const usernameField = useRef<HTMLInputElement>(null);

  useEffect(() => {
    usernameField.current?.focus();
    const close = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", close);
    return () => document.removeEventListener("keydown", close);
  }, [onClose]);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!username.trim() || !password) {
      setProblem("credentials");
      return;
    }
    setPending(true);
    setProblem(null);
    try {
      const view = await gateway.signIn(username.trim(), password);
      setPassword("");
      onSignedIn(view);
    } catch (error) {
      setProblem(deliveryFailure(error).problem);
    } finally {
      setPending(false);
    }
  };

  return createPortal(
    <div className="delivery-dialog-backdrop">
      <section
        className="delivery-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="delivery-sign-in-title"
        aria-describedby="delivery-sign-in-lead"
      >
        <div className="delivery-dialog-banner" aria-hidden="true">
          <span className="delivery-dialog-emblem"><Archive /></span>
          <span className="delivery-dialog-tab">DLA <small>アーカイブ</small></span>
        </div>
        <button
          className="delivery-icon-button delivery-dialog-close"
          type="button"
          onClick={onClose}
          aria-label={t("common.cancel")}
        >
          <X aria-hidden="true" />
        </button>
        <div className="delivery-dialog-body">
          <h2 id="delivery-sign-in-title">{t("delivery.signInTitle")}</h2>
          <p className="delivery-dialog-lead" id="delivery-sign-in-lead">{t("delivery.signInLead")}</p>
          <form className="delivery-form" onSubmit={submit} noValidate>
            <label>
              <span>{t("delivery.username")}</span>
              <span className="delivery-field">
                <UserRound aria-hidden="true" />
                <input
                  ref={usernameField}
                  autoComplete="username"
                  spellCheck={false}
                  maxLength={80}
                  disabled={pending}
                  value={username}
                  onChange={(event) => setUsername(event.target.value)}
                />
              </span>
            </label>
            <label>
              <span>{t("delivery.password")}</span>
              <span className="delivery-field">
                <KeyRound aria-hidden="true" />
                <input
                  type={showPassword ? "text" : "password"}
                  autoComplete="current-password"
                  maxLength={200}
                  disabled={pending}
                  value={password}
                  onChange={(event) => setPassword(event.target.value)}
                />
                <button
                  className="delivery-field-reveal"
                  type="button"
                  aria-pressed={showPassword}
                  aria-label={showPassword ? t("delivery.hidePassword") : t("delivery.showPassword")}
                  title={showPassword ? t("delivery.hidePassword") : t("delivery.showPassword")}
                  onClick={() => setShowPassword((shown) => !shown)}
                >
                  {showPassword ? <EyeOff aria-hidden="true" /> : <Eye aria-hidden="true" />}
                </button>
              </span>
            </label>
            {problem && (
              <p className="delivery-problem delivery-sign-in-problem" role="alert">
                <CircleAlert aria-hidden="true" />
                {problemText(t, problem)}
              </p>
            )}
            <div className="delivery-dialog-actions">
              <button className="delivery-button" type="button" onClick={onClose} disabled={pending}>
                {t("common.cancel")}
              </button>
              <button className="delivery-sign-in-submit" type="submit" disabled={pending}>
                {pending && <LoaderCircle className="delivery-spin" aria-hidden="true" />}
                {pending ? t("delivery.signingIn") : t("delivery.signIn")}
                {!pending && <ArrowRight className="delivery-sign-in-arrow" aria-hidden="true" />}
              </button>
            </div>
          </form>
          {onOpenExternal && (
            <p className="delivery-dialog-plans">
              <span>{t("delivery.plansPrompt")}</span>
              <button
                className="delivery-link"
                type="button"
                onClick={() => void onOpenExternal(patronPlansUrl(locale))}
              >
                {t("delivery.plansLink")}
              </button>
            </p>
          )}
        </div>
      </section>
    </div>,
    document.body,
  );
}
