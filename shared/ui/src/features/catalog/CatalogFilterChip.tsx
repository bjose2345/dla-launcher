import { Minus, Plus, X } from "lucide-react";

import { usePresentation } from "../../preferences/PresentationProvider";

import type { CatalogFacetState } from "./types";

export function CatalogFilterChip({
  label,
  count,
  state,
  onCycle,
  onRemove,
}: {
  label: string;
  count?: number;
  state: CatalogFacetState;
  onCycle: () => void;
  onRemove?: () => void;
}) {
  const { locale, t } = usePresentation();
  const stateIcon = state === "include"
    ? <Plus aria-hidden="true" />
    : state === "exclude"
      ? <Minus aria-hidden="true" />
      : null;

  if (onRemove) {
    return (
      <span className={`catalog-filter-chip selected ${state}`}>
        <button
          type="button"
          onClick={onCycle}
          title={t(state === "include" ? "filter.exclude" : "filter.include")}
        >
          {stateIcon}
          <span>{label}</span>
        </button>
        <button type="button" onClick={onRemove} aria-label={t("filter.remove", { label })}>
          <X aria-hidden="true" />
        </button>
      </span>
    );
  }

  return (
    <button
      type="button"
      className={`catalog-filter-chip ${state}`}
      aria-pressed={state !== "off"}
      onClick={onCycle}
      title={t(state === "off" ? "filter.include" : state === "include" ? "filter.exclude" : "filter.clear")}
    >
      {stateIcon}
      <span>{label}</span>
      {count !== undefined && <small>{count.toLocaleString(locale)}</small>}
    </button>
  );
}
