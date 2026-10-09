import {
  AppShell,
  type AppShellNavItem,
  type ReadOnlyDeepLinkTarget,
} from "@dla-launcher/shared-ui/app";
import {
  CatalogPage,
  catalogRouteSearch,
  defaultCatalogRouteState,
  parseCatalogSearch,
} from "@dla-launcher/shared-ui/catalog";
import { DeliveryActivity } from "@dla-launcher/shared-ui/delivery/shell";
import {
  ActiveLaunchPill,
  MediaDock,
  ReaderOverlay,
  manageTabs,
  type ManageTab,
} from "@dla-launcher/shared-ui/library/shell";
import { parseSettingsSearch, usePresentation } from "@dla-launcher/shared-ui/preferences/shell";
import { ScanInstallActivity } from "@dla-launcher/shared-ui/scanner/shell";
import { SupportRecoveryNotice } from "@dla-launcher/shared-ui/support/shell";
import { lazy, Suspense, useCallback, type ReactNode } from "react";
import {
  createRootRoute,
  createRoute,
  createRouter,
  redirect,
  useMatch,
  useNavigate,
} from "@tanstack/react-router";
import { openUrl } from "@tauri-apps/plugin-opener";

import { tauriCatalogGateway } from "./gateways/tauriCatalogGateway";
import {
  tauriAndroidAppGateway,
  tauriAndroidPackageGateway,
} from "./gateways/tauriAndroidPackageGateway";
import { tauriCoverCacheGateway } from "./gateways/tauriCoverCacheGateway";
import { tauriDeliveryGateway } from "./gateways/tauriDeliveryGateway";
import { tauriCatalogImportGateway } from "./gateways/tauriCatalogImportGateway";
import { tauriSearchGateway } from "./gateways/tauriSearchGateway";
import { tauriScannerGateway } from "./gateways/tauriScannerGateway";
import { tauriLibraryGateway } from "./gateways/tauriLibraryGateway";
import { tauriSupportGateway } from "./gateways/tauriSupportGateway";
import { tauriSystemGateway } from "./gateways/tauriSystemGateway";
import { includedSoftware } from "./includedSoftware";

// The catalog is the default route and stays in the startup chunk; every other
// page loads when it is first opened.
const WorkDetailPage = lazy(() =>
  import("@dla-launcher/shared-ui/catalog/detail").then((module) => ({ default: module.WorkDetailPage })),
);
const AboutPage = lazy(() =>
  import("@dla-launcher/shared-ui/about").then((module) => ({ default: module.AboutPage })),
);
const AndroidPackagePage = lazy(() =>
  import("@dla-launcher/shared-ui/android-package").then((module) => ({ default: module.AndroidPackagePage })),
);
const CatalogImportPage = lazy(() =>
  import("@dla-launcher/shared-ui/importer").then((module) => ({ default: module.CatalogImportPage })),
);
const DownloadsPage = lazy(() =>
  import("@dla-launcher/shared-ui/delivery").then((module) => ({ default: module.DownloadsPage })),
);
const LibraryPage = lazy(() =>
  import("@dla-launcher/shared-ui/library/page").then((module) => ({ default: module.LibraryPage })),
);
const MediaSessionPage = lazy(() =>
  import("@dla-launcher/shared-ui/library/media").then((module) => ({ default: module.MediaSessionPage })),
);
const ScannerPage = lazy(() =>
  import("@dla-launcher/shared-ui/scanner").then((module) => ({ default: module.ScannerPage })),
);
const SettingsPage = lazy(() =>
  import("@dla-launcher/shared-ui/preferences").then((module) => ({ default: module.SettingsPage })),
);
const SupportPage = lazy(() =>
  import("@dla-launcher/shared-ui/support").then((module) => ({ default: module.SupportPage })),
);

const rootRoute = createRootRoute({
  component: RootRoute,
});

const catalogRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  validateSearch: parseCatalogSearch,
  component: CatalogRoute,
});

const aboutRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/about",
  component: AboutRoute,
});

function AboutRoute() {
  return (
    <LazyRoute>
      <AboutPage
        systemGateway={tauriSystemGateway}
        windowGateway={tauriSystemGateway}
        onOpenProject={() => tauriSupportGateway.openProject()}
        version={`v${__DLA_LAUNCHER_VERSION__}`}
        includedSoftware={__DLA_TARGET_PLATFORM__ === "android" ? [] : includedSoftware}
      />
    </LazyRoute>
  );
}

const settingsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/settings",
  validateSearch: parseSettingsSearch,
  beforeLoad: ({ search }) => {
    if (search.legacyAbout) throw redirect({ to: "/about", replace: true });
  },
  component: SettingsRoute,
});

function SettingsRoute() {
  const { tab } = settingsRoute.useSearch();
  const navigate = settingsRoute.useNavigate();
  return (
    <LazyRoute>
      <SettingsPage
        workPreferenceGateway={tauriLibraryGateway}
        coverCacheGateway={tauriCoverCacheGateway}
        scannerRootGateway={tauriScannerGateway}
        windowGateway={tauriSystemGateway}
        tab={tab}
        onOpenWork={(code) => navigate({ to: "/works/$code", params: { code } })}
        onTabChange={(next) => navigate({ search: { tab: next } })}
      />
    </LazyRoute>
  );
}

const supportRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/support",
  component: () => (
    <LazyRoute>
      <SupportPage gateway={tauriSupportGateway} />
    </LazyRoute>
  ),
});

const scannerRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/scanner",
  component: ScannerRoute,
});

const libraryRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/library",
  validateSearch: (search: Record<string, unknown>): { show?: string } => ({
    show: typeof search.show === "string" && search.show.length <= 512 ? search.show : undefined,
  }),
  component: LibraryRoute,
});

const libraryManageRoute = createRoute({
  getParentRoute: () => libraryRoute,
  path: "$installationId",
  validateSearch: (search: Record<string, unknown>): { intent?: "install"; tab?: ManageTab } => ({
    intent: search.intent === "install" ? "install" as const : undefined,
    tab: manageTabs.find((tab) => tab === search.tab),
  }),
});

const libraryMediaRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/library/$installationId/media",
  component: LibraryMediaRoute,
});

const importRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/import",
  component: ImportRoute,
});

const androidPackageRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/android-packages",
  validateSearch: (search: Record<string, unknown>) => ({
    work: typeof search.work === "string" && search.work.length > 0 && search.work.length <= 64
      ? search.work
      : undefined,
  }),
  component: AndroidPackageRoute,
});

// Install from DLA is offered on the desktop builds only.
const deliveryGateway = __DLA_TARGET_PLATFORM__ === "android" ? undefined : tauriDeliveryGateway;

const downloadsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/downloads",
  component: DownloadsRoute,
});

function DownloadsRoute() {
  const navigate = useNavigate();
  return (
    <LazyRoute>
      <DownloadsPage
        gateway={tauriDeliveryGateway}
        catalogGateway={tauriCatalogGateway}
        onOpenWork={(code) => navigate({ to: "/works/$code", params: { code } })}
        onOpenInstallation={(installationId) => navigate({ to: "/library", search: { show: installationId } })}
        onOpenExternal={openUrl}
      />
    </LazyRoute>
  );
}

const workRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/works/$code",
  component: WorkRoute,
});

const routeTree = rootRoute.addChildren([
  catalogRoute,
  scannerRoute,
  libraryRoute.addChildren([libraryManageRoute]),
  libraryMediaRoute,
  importRoute,
  androidPackageRoute,
  ...(deliveryGateway ? [downloadsRoute] : []),
  settingsRoute,
  supportRoute,
  aboutRoute,
  workRoute,
]);

export const router = createRouter({ routeTree });
let readOnlyDeepLinkNavigationPending = false;

export async function navigateReadOnlyDeepLink(target: ReadOnlyDeepLinkTarget) {
  readOnlyDeepLinkNavigationPending = true;
  try {
    await router.navigate({ to: "/works/$code", params: { code: target.code } });
  } finally {
    readOnlyDeepLinkNavigationPending = false;
  }
}

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}

function RootRoute() {
  const navigate = useNavigate();
  const { t } = usePresentation();
  const showAndroidPackages = __DLA_TARGET_PLATFORM__ === "android";
  return (
    <>
      <AppShell
      candidate={t("app.candidate")}
      navigation={launcherNavigation(t, showAndroidPackages)}
      launchIndicator={(
        <ActiveLaunchPill
          gateway={tauriLibraryGateway}
          onOpenLibrary={() => navigate({ to: "/library" })}
        />
      )}
      mediaDock={(
        <MediaDock
          onExpand={(installationId) => navigate({
            to: "/library/$installationId/media",
            params: { installationId },
          })}
        />
      )}
      readerOverlay={<ReaderOverlay gateway={tauriLibraryGateway} />}
      sidebarActivity={(
        <>
          {deliveryGateway ? (
            <DeliveryActivity
              gateway={deliveryGateway}
              catalogGateway={tauriCatalogGateway}
              onOpenDownloads={() => navigate({ to: "/downloads" })}
              onOpenInstallation={(installationId) => navigate({ to: "/library", search: { show: installationId } })}
            />
          ) : null}
          <ScanInstallActivity
            gateway={tauriScannerGateway}
            progressGateway={tauriLibraryGateway}
            catalogGateway={tauriCatalogGateway}
            onOpenScanner={() => navigate({ to: "/scanner" })}
            onShowInLibrary={(installationId) => navigate({ to: "/library", search: { show: installationId } })}
          />
        </>
      )}
      onOpenSettings={() => navigate({ to: "/settings", search: { tab: "general" } })}
      search={{
        gateway: tauriSearchGateway,
        onOpenWork: (code) => navigate({ to: "/works/$code", params: { code } }),
      }}
      />
      <SupportRecoveryNotice
        gateway={tauriSupportGateway}
        onOpenSupport={() => navigate({ to: "/support" })}
      />
    </>
  );
}

function launcherNavigation(
  t: ReturnType<typeof usePresentation>["t"],
  showAndroidPackages: boolean,
): AppShellNavItem[] {
  const items: AppShellNavItem[] = [
    { to: "/", label: t("nav.catalog"), icon: "catalog", exact: true },
    { to: "/library", label: t("nav.library"), icon: "library" },
    { to: "/scanner", label: t("nav.scanner"), icon: "scanner" },
    { to: "/import", label: t("nav.import"), icon: "import" },
    ...(deliveryGateway
      ? [{ to: "/downloads", label: t("nav.downloads"), icon: "downloads" } satisfies AppShellNavItem]
      : []),
    { to: "/settings", label: t("nav.settings"), icon: "settings", group: "secondary" },
    { to: "/support", label: t("nav.support"), icon: "support", group: "secondary" },
    { to: "/about", label: t("nav.about"), icon: "about", group: "secondary" },
  ];
  if (showAndroidPackages) {
    items.splice(4, 0, {
      to: "/android-packages",
      label: t("nav.androidPackages"),
      icon: "androidPackage",
    });
  }
  return items;
}

function ScannerRoute() {
  const navigate = scannerRoute.useNavigate();
  return (
    <LazyRoute>
      <ScannerPage
        gateway={tauriScannerGateway}
        libraryGateway={tauriLibraryGateway}
        catalogGateway={tauriCatalogGateway}
        searchGateway={tauriSearchGateway}
        onShowInLibrary={(installationId) => navigate({ to: "/library", search: { show: installationId } })}
        onOpenLibrary={() => navigate({ to: "/library", search: {} })}
        onOpenWork={(code) => navigate({ to: "/works/$code", params: { code } })}
      />
    </LazyRoute>
  );
}

// The Manage panel opens over the Library, which keeps its lens and view.
function LibraryRoute() {
  const navigate = useNavigate();
  const { show } = libraryRoute.useSearch();
  const manage = useMatch({ from: "/library/$installationId", shouldThrow: false });
  return (
    <LazyRoute>
      <LibraryPage
        gateway={tauriLibraryGateway}
        androidAppGateway={__DLA_TARGET_PLATFORM__ === "android" ? tauriAndroidAppGateway : undefined}
        catalogGateway={tauriCatalogGateway}
        searchGateway={tauriSearchGateway}
        installs={__DLA_TARGET_PLATFORM__ === "android" ? undefined : tauriScannerGateway}
        manage={manage
          ? {
            installationId: manage.params.installationId,
            tab: manage.search.tab ?? (manage.search.intent === "install" ? "files" : undefined),
          }
          : undefined}
        spotlight={show}
        onCloseManage={() => navigate({ to: "/library", search: { show } })}
        onManage={(installationId, tab) =>
          navigate({
            to: "/library/$installationId",
            params: { installationId },
            search: { show, tab },
          })
        }
        onOpenMedia={(installationId) =>
          navigate({
            to: "/library/$installationId/media",
            params: { installationId },
          })
        }
        onOpenWork={(code) => navigate({ to: "/works/$code", params: { code } })}
        onReinstallAndroidApp={(work) => navigate({
          to: "/android-packages",
          search: { work },
        })}
      />
    </LazyRoute>
  );
}

function AndroidPackageRoute() {
  const { work } = androidPackageRoute.useSearch();
  const navigate = androidPackageRoute.useNavigate();
  return (
    <LazyRoute>
      <AndroidPackagePage
        gateway={tauriAndroidPackageGateway}
        associationGateway={tauriAndroidAppGateway}
        workCode={work}
        onOpenLibrary={() => navigate({ to: "/library" })}
      />
    </LazyRoute>
  );
}

function ImportRoute() {
  const navigate = importRoute.useNavigate();
  return (
    <LazyRoute>
      <CatalogImportPage
        gateway={tauriCatalogImportGateway}
        onOpenCatalog={() => navigate({ to: "/", search: catalogRouteSearch(defaultCatalogRouteState) })}
      />
    </LazyRoute>
  );
}

function LibraryMediaRoute() {
  const { installationId } = libraryMediaRoute.useParams();
  const navigate = libraryMediaRoute.useNavigate();
  return (
    <LazyRoute>
      <MediaSessionPage
        installationId={installationId}
        gateway={tauriLibraryGateway}
        catalogGateway={tauriCatalogGateway}
        onBack={() => navigate({ to: "/library" })}
      />
    </LazyRoute>
  );
}

function CatalogRoute() {
  const filters = catalogRoute.useSearch();
  const navigate = catalogRoute.useNavigate();
  const updateRoute = useCallback((
    change: Partial<Pick<typeof filters, "sort" | "timeline" | "month" | "page" | "day">>,
    replace = false,
  ) => {
    if (readOnlyDeepLinkNavigationPending || router.history.location.pathname !== "/") return;
    void navigate({
      search: (current) => ({ ...current, ...change }),
      replace,
    });
  }, [navigate]);

  return (
    <CatalogPage
      filters={filters}
      gateway={tauriCatalogGateway}
      onRouteChange={updateRoute}
      onOpenWork={(code) => navigate({ to: "/works/$code", params: { code } })}
    />
  );
}

function WorkRoute() {
  const { code } = workRoute.useParams();
  const navigate = workRoute.useNavigate();
  const { t } = usePresentation();
  const back = () => {
    if (window.history.length > 1) window.history.back();
    else void navigate({ to: "/", search: catalogRouteSearch(defaultCatalogRouteState) });
  };

  return (
    <Suspense fallback={<ModuleLoading label={t("detail.routeLoading")} />}>
      <WorkDetailPage
        code={code}
        gateway={tauriCatalogGateway}
        libraryGateway={tauriLibraryGateway}
        onBack={back}
        onOpenCatalog={() => navigate({ to: "/", search: catalogRouteSearch(defaultCatalogRouteState) })}
        onOpenWork={(nextCode) =>
          navigate({ to: "/works/$code", params: { code: nextCode } })
        }
        onOpenScanner={() => navigate({ to: "/scanner" })}
        onOpenInstallation={(installationId, intent) => navigate({
          to: "/library/$installationId",
          params: { installationId },
          search: { intent: intent === "install" ? "install" : undefined },
        })}
        onOpenMedia={(installationId) => navigate({
          to: "/library/$installationId/media",
          params: { installationId },
        })}
        deliveryGateway={deliveryGateway}
        onInstallAndroidApp={__DLA_TARGET_PLATFORM__ === "android"
          ? (work) => navigate({ to: "/android-packages", search: { work } })
          : undefined}
        onOpenExternal={openUrl}
      />
    </Suspense>
  );
}

function LazyRoute({ children }: { children: ReactNode }) {
  const { t } = usePresentation();
  return <Suspense fallback={<ModuleLoading label={t("app.routeLoading")} />}>{children}</Suspense>;
}

function ModuleLoading({ label }: { label: string }) {
  return (
    <main className="work-detail-loading" aria-live="polite" aria-label={label}>
      <div className="work-detail-loading-layout"><span /><span /></div>
      <div className="route-loading-overlay"><i /><strong>{label}</strong></div>
    </main>
  );
}
