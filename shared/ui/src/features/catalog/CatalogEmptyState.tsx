import { LayoutGrid } from "lucide-react";

import { usePresentation } from "../../preferences/PresentationProvider";
import type { CatalogSnapshot } from "./types";

export function catalogSnapshotIsEmpty(snapshot: CatalogSnapshot): boolean {
  return snapshot.realWorks + snapshot.syntheticWorks === 0;
}

export function CatalogEmptyState() {
  const { t } = usePresentation();

  return (
    <section className="catalog-empty catalog-empty-library">
      <LayoutGrid aria-hidden="true" />
      <h1>{t("catalog.emptyTitle")}</h1>
      <p>{t("catalog.emptyHelp")}</p>
    </section>
  );
}
