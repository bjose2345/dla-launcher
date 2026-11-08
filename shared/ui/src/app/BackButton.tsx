import { ArrowLeft, LoaderCircle } from "lucide-react";

import { usePresentation } from "../preferences/PresentationProvider";

/** The arrow that leaves a full-screen viewer or player, the same on each. */
export function BackButton({
  closing = false,
  className,
  onClick,
}: {
  closing?: boolean;
  className?: string;
  onClick: () => void;
}) {
  const { t } = usePresentation();
  return (
    <button
      className={className ? `app-back-button ${className}` : "app-back-button"}
      type="button"
      aria-label={t("library.back")}
      title={t("library.back")}
      disabled={closing}
      onClick={onClick}
    >
      {closing ? <LoaderCircle className="app-spin" aria-hidden="true" /> : <ArrowLeft aria-hidden="true" />}
    </button>
  );
}
