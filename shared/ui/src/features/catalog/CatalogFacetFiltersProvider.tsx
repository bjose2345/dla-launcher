import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useState,
  type Dispatch,
  type ReactNode,
  type SetStateAction,
} from "react";

import { readCatalogFacetFilters, writeCatalogFacetFilters } from "./catalogFilters";
import type { CatalogFacetFilters } from "./types";

interface CatalogFacetFiltersContextValue {
  filters: CatalogFacetFilters;
  setFilters: Dispatch<SetStateAction<CatalogFacetFilters>>;
}

const CatalogFacetFiltersContext = createContext<CatalogFacetFiltersContextValue | null>(null);

export function CatalogFacetFiltersProvider({ children }: { children: ReactNode }) {
  const [filters, setFilters] = useState(readCatalogFacetFilters);

  useEffect(() => writeCatalogFacetFilters(filters), [filters]);

  const value = useMemo(() => ({ filters, setFilters }), [filters]);
  return (
    <CatalogFacetFiltersContext.Provider value={value}>
      {children}
    </CatalogFacetFiltersContext.Provider>
  );
}

export function useCatalogFacetFilters(): CatalogFacetFiltersContextValue {
  const value = useContext(CatalogFacetFiltersContext);
  if (!value) {
    throw new Error("useCatalogFacetFilters must be used within CatalogFacetFiltersProvider");
  }
  return value;
}
