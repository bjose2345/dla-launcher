import { MoreHorizontal } from "lucide-react";
import { useRef, useState, type MouseEvent } from "react";

import { AnchoredPopover } from "./AnchoredPopover";

export interface ActionMenuItem {
  label: string;
  onSelect: () => void;
  danger?: boolean;
}

/** A "···" button that opens a short list of actions for one item. */
export function ActionMenu({
  label,
  items,
  className,
}: {
  /** Names the item, such as "More for KUNOICHI RED". */
  label: string;
  items: ActionMenuItem[];
  className?: string;
}) {
  const [open, setOpen] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  const toggle = (event: MouseEvent) => {
    event.stopPropagation();
    setOpen((current) => !current);
  };

  return (
    <>
      <button
        ref={button}
        className={`app-action-menu-button${className ? ` ${className}` : ""}`}
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={label}
        title={label}
        onClick={toggle}
      >
        <MoreHorizontal aria-hidden="true" />
      </button>
      {open ? (
        <AnchoredPopover
          anchorRef={button}
          className="app-action-menu"
          role="menu"
          ariaLabel={label}
          align="end"
          gap={6}
          maximumWidth={260}
          minimumWidth={200}
          onClose={() => setOpen(false)}
          onClick={(event) => event.stopPropagation()}
        >
          {items.map((item) => (
            <button
              className={item.danger ? "is-danger" : undefined}
              type="button"
              role="menuitem"
              key={item.label}
              onClick={() => {
                setOpen(false);
                item.onSelect();
              }}
            >
              {item.label}
            </button>
          ))}
        </AnchoredPopover>
      ) : null}
    </>
  );
}
