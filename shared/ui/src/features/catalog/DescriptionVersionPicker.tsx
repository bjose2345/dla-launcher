import { Check, ChevronDown } from "lucide-react";
import { useId, useRef, useState, type SyntheticEvent } from "react";

import { AnchoredPopover } from "../../app/AnchoredPopover";
import type { CatalogDescriptionVersion } from "./types";

interface DescriptionVersionPickerProps {
  versions: CatalogDescriptionVersion[];
  selectedVersion: number;
  latestVersion: number;
  latestLabel: string;
  onSelect: (version: number) => void;
}

export function DescriptionVersionPicker({
  versions,
  selectedVersion,
  latestVersion,
  latestLabel,
  onSelect,
}: DescriptionVersionPickerProps) {
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuId = useId();
  const selected = versions.find((entry) => entry.version === selectedVersion) ?? versions[0];

  const label = (version: number) => `V${version}${version === latestVersion ? ` (${latestLabel})` : ""}`;
  const containSummaryInteraction = (event: SyntheticEvent) => event.stopPropagation();

  return (
    <div
      className="description-version-picker"
      data-open={open}
      onClick={containSummaryInteraction}
      onKeyDown={containSummaryInteraction}
    >
      <button
        ref={triggerRef}
        type="button"
        className="description-version-trigger"
        aria-haspopup="listbox"
        aria-controls={menuId}
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
      >
        <span>{selected ? label(selected.version) : ""}</span>
        <ChevronDown aria-hidden="true" />
      </button>
      {open ? (
        <AnchoredPopover
          anchorRef={triggerRef}
          id={menuId}
          className="description-version-menu"
          role="listbox"
          minimumWidth={192}
          matchAnchorWidth
          maximumWidth={320}
          onClose={() => setOpen(false)}
        >
          {versions.map((entry) => {
            const active = entry.version === selectedVersion;
            return (
              <button
                key={entry.version}
                type="button"
                role="option"
                aria-selected={active}
                onClick={() => {
                  onSelect(entry.version);
                  setOpen(false);
                }}
              >
                <span>{label(entry.version)}</span>
                <Check aria-hidden="true" data-visible={active} />
              </button>
            );
          })}
        </AnchoredPopover>
      ) : null}
    </div>
  );
}
