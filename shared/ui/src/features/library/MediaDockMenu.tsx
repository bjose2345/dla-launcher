import { useRef, type ReactNode } from "react";
import { Check, MoreHorizontal } from "lucide-react";

import { AnchoredPopover } from "../../app/AnchoredPopover";
import { usePresentation } from "../../preferences/PresentationProvider";
import type { MessageKey } from "../../preferences/preferences";

export const PLAYBACK_RATES = [0.5, 0.75, 1, 1.25, 1.5, 2] as const;

export type PlaybackRate = (typeof PLAYBACK_RATES)[number];

export function formatPlaybackRate(rate: number): string {
  return `${Number.isInteger(rate) ? rate : rate.toString()}×`;
}

export function MediaDockMenu({
  label,
  children,
  icon,
  active = false,
  open,
  onOpenChange,
}: {
  label: string;
  children: ReactNode;
  icon?: ReactNode;
  active?: boolean;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const { t } = usePresentation();
  const triggerRef = useRef<HTMLButtonElement>(null);

  return (
    <div className={`media-dock-menu${open ? " is-open" : ""}`}>
      <button
        ref={triggerRef}
        type="button"
        aria-expanded={open}
        aria-haspopup="menu"
        aria-pressed={active}
        className={active ? "is-active" : undefined}
        title={label}
        aria-label={label}
        onClick={() => onOpenChange(!open)}
      >
        {icon ?? <MoreHorizontal aria-hidden="true" />}
      </button>
      {open ? (
        <AnchoredPopover
          anchorRef={triggerRef}
          className="media-dock-menu-panel"
          role="menu"
          ariaLabel={label}
          side="top"
          align="end"
          maximumWidth={1040}
          onClose={() => onOpenChange(false)}
          onClick={(event) => {
            if (!(event.target as Element).closest('[role="menuitemradio"]')) return;
            onOpenChange(false);
          }}
        >
          {children}
        </AnchoredPopover>
      ) : null}
      <span className="media-dock-menu-live" aria-live="polite">
        {open ? t("media.menu.opened") : ""}
      </span>
    </div>
  );
}

export function MediaDockMenuGroup({
  labelKey,
  children,
}: {
  labelKey: MessageKey;
  children: ReactNode;
}) {
  const { t } = usePresentation();
  return (
    <div className="media-dock-menu-group" role="group" aria-label={t(labelKey)}>
      <span>{t(labelKey)}</span>
      <div>{children}</div>
    </div>
  );
}

export function MediaDockMenuItem({
  active,
  label,
  onSelect,
}: {
  active: boolean;
  label: string;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      role="menuitemradio"
      aria-checked={active}
      className={active ? "is-active" : undefined}
      onClick={onSelect}
    >
      <span>{label}</span>
      {active ? <Check aria-hidden="true" /> : null}
    </button>
  );
}
