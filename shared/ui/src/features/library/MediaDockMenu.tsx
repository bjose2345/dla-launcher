import {
  Children,
  createContext,
  isValidElement,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactElement,
  type ReactNode,
} from "react";
import { Check, ChevronLeft, ChevronRight, MoreHorizontal } from "lucide-react";

import { AnchoredPopover } from "../../app/AnchoredPopover";
import { usePresentation } from "../../preferences/PresentationProvider";
import type { MessageKey } from "../../preferences/preferences";

export const PLAYBACK_RATES = [0.5, 0.75, 1, 1.25, 1.5, 2] as const;

export type PlaybackRate = (typeof PLAYBACK_RATES)[number];

export function formatPlaybackRate(rate: number): string {
  return `${Number.isInteger(rate) ? rate : rate.toString()}×`;
}

/**
 * The dock's settings menu, laid out as a settings list: one row per setting
 * showing its current value, whose choices open in place behind a back
 * header; toggles switch in place; actions act and close. The list never
 * widens with the number of choices.
 */
interface MenuSection {
  section: string | null;
  openSection: (section: string | null) => void;
}

const MenuSectionContext = createContext<MenuSection>({ section: null, openSection: () => undefined });

export function MediaDockMenu({
  label,
  children,
  icon,
  active = false,
  open,
  gap,
  onOpenChange,
}: {
  label: string;
  children: ReactNode;
  icon?: ReactNode;
  active?: boolean;
  open: boolean;
  gap?: number;
  onOpenChange: (open: boolean) => void;
}) {
  const { t } = usePresentation();
  const triggerRef = useRef<HTMLButtonElement>(null);
  const [section, setSection] = useState<string | null>(null);
  useEffect(() => {
    if (!open) setSection(null);
  }, [open]);

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
          gap={gap}
          maximumWidth={360}
          onClose={() => onOpenChange(false)}
          onClick={(event) => {
            const target = event.target as Element;
            // Rows that open a section or switch a toggle keep the menu open;
            // a choice or an action is the end of the visit.
            if (target.closest("[data-menu-keep]")) return;
            if (!target.closest('[role="menuitemradio"], [role="menuitem"]')) return;
            onOpenChange(false);
          }}
        >
          <MenuSectionContext.Provider value={{ section, openSection: setSection }}>
            {children}
          </MenuSectionContext.Provider>
        </AnchoredPopover>
      ) : null}
      <span className="media-dock-menu-live" aria-live="polite">
        {open ? t("media.menu.opened") : ""}
      </span>
    </div>
  );
}

/** A setting with several choices: a row with its current value that opens them. */
export function MediaDockMenuGroup({
  labelKey,
  children,
}: {
  labelKey: MessageKey;
  children: ReactNode;
}) {
  const { t } = usePresentation();
  const { section, openSection } = useContext(MenuSectionContext);
  const items = Children.toArray(children).filter(isValidElement) as ReactElement<{ active?: boolean; label?: string; swatch?: string }>[];
  const current = items.find((item) => item.props.active)?.props ?? null;

  if (section === labelKey) {
    return (
      <div className="media-dock-menu-group" role="group" aria-label={t(labelKey)}>
        <button
          type="button"
          className="media-dock-menu-back"
          data-menu-keep=""
          aria-label={t("common.back")}
          onClick={() => openSection(null)}
        >
          <ChevronLeft aria-hidden="true" />
          <span>{t(labelKey)}</span>
        </button>
        <div>{children}</div>
      </div>
    );
  }
  if (section !== null) return null;
  return (
    <button
      type="button"
      role="menuitem"
      aria-haspopup="menu"
      className="media-dock-menu-row"
      data-menu-keep=""
      aria-label={current?.label ? `${t(labelKey)}, ${current.label}` : t(labelKey)}
      onClick={() => openSection(labelKey)}
    >
      <span className="media-dock-menu-row-label">{t(labelKey)}</span>
      {current?.label ? (
        <span className="media-dock-menu-row-value">
          {current.swatch ? <i aria-hidden="true" style={{ background: current.swatch }} /> : null}
          {current.label}
        </span>
      ) : null}
      <ChevronRight aria-hidden="true" />
    </button>
  );
}

/**
 * One choice of a group. Choosing it closes the menu, unless the choice is
 * something to look at (a subtitle style): then the menu returns to its list
 * so the next setting is one press away.
 */
export function MediaDockMenuItem({
  active,
  label,
  swatch,
  keepOpen = false,
  onSelect,
}: {
  active: boolean;
  label: string;
  swatch?: string;
  keepOpen?: boolean;
  onSelect: () => void;
}) {
  const { openSection } = useContext(MenuSectionContext);
  return (
    <button
      type="button"
      role="menuitemradio"
      aria-checked={active}
      className={`media-dock-menu-row is-choice${active ? " is-active" : ""}`}
      data-menu-keep={keepOpen ? "" : undefined}
      onClick={() => {
        onSelect();
        if (keepOpen) openSection(null);
      }}
    >
      <span className="media-dock-menu-row-label">
        {swatch ? <i aria-hidden="true" style={{ background: swatch }} /> : null}
        {label}
      </span>
      {active ? <Check aria-hidden="true" /> : <span aria-hidden="true" />}
    </button>
  );
}

/** An on/off setting, switched in place. */
export function MediaDockMenuToggle({
  label,
  checked,
  onToggle,
}: {
  label: string;
  checked: boolean;
  onToggle: (checked: boolean) => void;
}) {
  const { t } = usePresentation();
  const { section } = useContext(MenuSectionContext);
  if (section !== null) return null;
  return (
    <button
      type="button"
      role="menuitemcheckbox"
      aria-checked={checked}
      className={`media-dock-menu-row is-toggle${checked ? " is-active" : ""}`}
      data-menu-keep=""
      aria-label={label}
      onClick={() => onToggle(!checked)}
    >
      <span className="media-dock-menu-row-label">{label}</span>
      <span className="media-dock-menu-row-value">{t(checked ? "common.on" : "common.off")}</span>
      <span className="media-dock-menu-switch" aria-hidden="true" />
    </button>
  );
}

/** Something to do, not to set; doing it closes the menu. */
export function MediaDockMenuAction({
  label,
  onSelect,
}: {
  label: string;
  onSelect: () => void;
}) {
  const { section } = useContext(MenuSectionContext);
  if (section !== null) return null;
  return (
    <button type="button" role="menuitem" className="media-dock-menu-row is-action" onClick={onSelect}>
      <span className="media-dock-menu-row-label">{label}</span>
    </button>
  );
}
