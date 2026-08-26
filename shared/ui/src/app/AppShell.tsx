import { Link, Outlet } from "@tanstack/react-router";
import {
  Download,
  FolderSearch,
  Info,
  LayoutGrid,
  Library,
  PanelLeftClose,
  PanelLeftOpen,
  PackageOpen,
  Search,
  Settings,
  LifeBuoy,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";

import {
  catalogRouteSearch,
  defaultCatalogRouteState,
} from "../features/catalog/query";
import { CatalogFacetFiltersProvider } from "../features/catalog/CatalogFacetFiltersProvider";
import { SearchModal, type SearchGateway } from "../features/search";
import { useBoundKeys, useKeyBindings } from "../preferences/KeyBindingsProvider";
import { ariaKeyShortcuts, bindingsForAction, formatCombo } from "../preferences/keyBindings";
import { usePresentation } from "../preferences/PresentationProvider";
import { BrandMark } from "./BrandMark";

const defaultSearch = catalogRouteSearch(defaultCatalogRouteState);
const narrowViewportQuery = "(max-width: 900px)";

const navIcons = {
  catalog: LayoutGrid,
  library: Library,
  scanner: FolderSearch,
  import: Download,
  androidPackage: PackageOpen,
  settings: Settings,
  support: LifeBuoy,
  about: Info,
} as const;

export type AppShellNavIcon = keyof typeof navIcons;

export interface AppShellNavItem {
  to: string;
  label: string;
  icon: AppShellNavIcon;
  exact?: boolean;
  group?: "primary" | "secondary";
}

interface AppShellProps {
  candidate: string;
  navigation?: AppShellNavItem[];
  launchIndicator?: ReactNode;
  mediaDock?: ReactNode;
  readerOverlay?: ReactNode;
  onOpenSettings?: () => void;
  search?: {
    gateway: SearchGateway;
    onOpenWork: (code: string) => void | Promise<void>;
  };
}

export function AppShell(props: AppShellProps) {
  return (
    <CatalogFacetFiltersProvider>
      <AppFrame {...props} />
    </CatalogFacetFiltersProvider>
  );
}

function AppFrame({
  candidate,
  navigation = [],
  launchIndicator,
  mediaDock,
  readerOverlay,
  onOpenSettings,
  search,
}: AppShellProps) {
  const { sidebarCollapsed, setSidebarCollapsed, t } = usePresentation();
  const { bindings } = useKeyBindings();
  const [searchOpen, setSearchOpen] = useState(false);
  const narrow = useNarrowViewport();
  const collapsed = sidebarCollapsed || narrow;
  const toggleSidebar = useCallback(
    () => {
      if (!narrow) setSidebarCollapsed(!sidebarCollapsed);
    },
    [narrow, setSidebarCollapsed, sidebarCollapsed],
  );

  const globalHandlers = useMemo(() => ({
    search: search ? () => setSearchOpen(true) : undefined,
    toggleSidebar: navigation.length > 0 && !narrow ? toggleSidebar : undefined,
    openSettings: onOpenSettings,
  }), [narrow, navigation.length, onOpenSettings, search, toggleSidebar]);
  useBoundKeys("global", globalHandlers);

  const searchBindings = bindingsForAction(bindings, "search");
  const toggleBindings = bindingsForAction(bindings, "toggleSidebar");

  const primary = navigation.filter((item) => item.group !== "secondary");
  const secondary = navigation.filter((item) => item.group === "secondary");

  return (
    <div className="app-frame">
      <header className="app-topbar">
        <div className="app-topbar-brand">
          {navigation.length > 0 && !narrow ? (
            <button
              className="app-sidebar-toggle"
              type="button"
              aria-expanded={!collapsed}
              aria-keyshortcuts={ariaKeyShortcuts(toggleBindings)}
              aria-label={t(collapsed ? "nav.expandSidebar" : "nav.collapseSidebar")}
              title={t(collapsed ? "nav.expandSidebar" : "nav.collapseSidebar")}
              onClick={toggleSidebar}
            >
              {collapsed
                ? <PanelLeftOpen aria-hidden="true" />
                : <PanelLeftClose aria-hidden="true" />}
            </button>
          ) : null}
          <Link className="brand" to="/" search={defaultSearch} title={candidate} aria-label={t("nav.home")}>
            <BrandMark />
          </Link>
        </div>

        {search ? (
          <button
            className="app-search-trigger"
            type="button"
            aria-label={t("search.openCommand")}
            aria-keyshortcuts={ariaKeyShortcuts(searchBindings)}
            onClick={() => setSearchOpen(true)}
          >
            <Search aria-hidden="true" />
            <span>{t("nav.search")}</span>
            {searchBindings[0] ? <kbd>{formatCombo(searchBindings[0])}</kbd> : null}
          </button>
        ) : <span className="app-search-spacer" />}

        <div className="app-topbar-actions">{launchIndicator}</div>
      </header>

      <div className="app-body">
        {navigation.length ? (
          <aside className="app-sidebar" data-collapsed={collapsed ? "true" : "false"}>
            <nav aria-label={t("nav.primary")}>
              {primary.map((item) => (
                <SidebarLink item={item} collapsed={collapsed} key={item.to} />
              ))}
            </nav>
            {secondary.length ? (
              <nav className="app-sidebar-secondary" aria-label={t("nav.secondary")}>
                {secondary.map((item) => (
                  <SidebarLink item={item} collapsed={collapsed} key={item.to} />
                ))}
              </nav>
            ) : null}
          </aside>
        ) : null}

        <Outlet />
      </div>

      {mediaDock}
      {readerOverlay}
      {search ? (
        <SearchModal
          open={searchOpen}
          gateway={search.gateway}
          onClose={() => setSearchOpen(false)}
          onOpenWork={search.onOpenWork}
        />
      ) : null}
    </div>
  );
}

function SidebarLink({ item, collapsed }: { item: AppShellNavItem; collapsed: boolean }) {
  const { t } = usePresentation();
  const Icon = navIcons[item.icon];
  return (
    <Link
      className="app-nav-item"
      to={item.to}
      search={item.to === "/" ? defaultSearch : undefined}
      activeOptions={item.exact ? { exact: true } : undefined}
      activeProps={{ className: "app-nav-item active" }}
      aria-label={item.label}
      title={collapsed ? item.label : undefined}
    >
      <span className="app-nav-icon"><Icon aria-hidden="true" /></span>
      <span className="app-nav-label">{item.label}</span>
    </Link>
  );
}

function useNarrowViewport(): boolean {
  const [narrow, setNarrow] = useState(() => (
    typeof window !== "undefined" && typeof window.matchMedia === "function"
      ? window.matchMedia(narrowViewportQuery).matches
      : false
  ));

  useEffect(() => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") return;
    const media = window.matchMedia(narrowViewportQuery);
    const onChange = (event: MediaQueryListEvent) => setNarrow(event.matches);
    setNarrow(media.matches);
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, []);

  return narrow;
}
