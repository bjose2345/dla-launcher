import { AboutPage } from "@dla-launcher/shared-ui/about";
import { AndroidPackagePage } from "@dla-launcher/shared-ui/android-package";
import {
  AppShell,
  isPreReleaseVersion,
  type AppShellNavItem,
  type ReadOnlyDeepLinkTarget,
} from "@dla-launcher/shared-ui/app";
import {
  CatalogPage,
  catalogRouteSearch,
  defaultCatalogRouteState,
  parseCatalogSearch,
} from "@dla-launcher/shared-ui/catalog";
import { DiagnosticsPage } from "@dla-launcher/shared-ui/diagnostics";
import { CatalogImportPage } from "@dla-launcher/shared-ui/importer";
import {
  ActiveLaunchPill,
  LibraryPage,
  MediaDock,
  ReaderOverlay,
  LibraryReviewPage,
  MediaSessionPage,
} from "@dla-launcher/shared-ui/library";
import {
  SettingsPage,
  parseSettingsSearch,
  usePresentation,
} from "@dla-launcher/shared-ui/preferences";
import { ScannerPage } from "@dla-launcher/shared-ui/scanner";
import { SupportPage, SupportRecoveryNotice } from "@dla-launcher/shared-ui/support";
import { lazy, Suspense, useCallback } from "react";
import {
  createRootRoute,
  createRoute,
  createRouter,
  redirect,
  useNavigate,
} from "@tanstack/react-router";
import { openUrl } from "@tauri-apps/plugin-opener";

import { tauriCatalogGateway } from "./gateways/tauriCatalogGateway";
import {
  tauriAndroidAppGateway,
  tauriAndroidPackageGateway,
} from "./gateways/tauriAndroidPackageGateway";
import { tauriCoverCacheGateway } from "./gateways/tauriCoverCacheGateway";
import { tauriDiagnosticsGateway } from "./gateways/tauriDiagnosticsGateway";
import { tauriCatalogImportGateway } from "./gateways/tauriCatalogImportGateway";
import { tauriSearchGateway } from "./gateways/tauriSearchGateway";
import { tauriScannerGateway } from "./gateways/tauriScannerGateway";
import { tauriLibraryGateway } from "./gateways/tauriLibraryGateway";
import { tauriSupportGateway } from "./gateways/tauriSupportGateway";

const WorkDetailPage = lazy(() =>
  import("@dla-launcher/shared-ui/catalog/detail").then((module) => ({ default: module.WorkDetailPage })),
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

const diagnosticsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/diagnostics",
  component: DiagnosticsRoute,
});

function DiagnosticsRoute() {
  const { t } = usePresentation();
  return (
    <DiagnosticsPage
      gateway={tauriDiagnosticsGateway}
      bridgeDescription={t("diagnostics.bridgeDescription")}
      platformNote={t("diagnostics.platformNote")}
    />
  );
}

const aboutRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/about",
  component: AboutRoute,
});

function AboutRoute() {
  return (
    <AboutPage
      systemGateway={tauriDiagnosticsGateway}
      windowGateway={tauriDiagnosticsGateway}
      onOpenProject={() => tauriSupportGateway.openProject()}
      version={`v${__DLA_LAUNCHER_VERSION__}`}
    />
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
    <SettingsPage
      workPreferenceGateway={tauriLibraryGateway}
      coverCacheGateway={tauriCoverCacheGateway}
      scannerRootGateway={tauriScannerGateway}
      windowGateway={tauriDiagnosticsGateway}
      tab={tab}
      onOpenWork={(code) => navigate({ to: "/works/$code", params: { code } })}
      onTabChange={(next) => navigate({ search: { tab: next } })}
    />
  );
}

const supportRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/support",
  component: () => <SupportPage gateway={tauriSupportGateway} />,
});

const scannerRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/scanner",
  component: ScannerRoute,
});

const libraryRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/library",
  component: LibraryRoute,
});

const libraryReviewRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/library/$installationId",
  validateSearch: (search: Record<string, unknown>) => ({
    intent: search.intent === "install" ? "install" as const : undefined,
  }),
  component: LibraryReviewRoute,
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

const workRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/works/$code",
  component: WorkRoute,
});

const routeTree = rootRoute.addChildren([
  catalogRoute,
  scannerRoute,
  libraryRoute,
  libraryReviewRoute,
  libraryMediaRoute,
  importRoute,
  androidPackageRoute,
  diagnosticsRoute,
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
  const version = `v${__DLA_LAUNCHER_VERSION__}`;
  const showAndroidPackages = __DLA_TARGET_PLATFORM__ === "android";
  return (
    <>
      <AppShell
      candidate={t("app.candidate")}
      navigation={launcherNavigation(t, isPreReleaseVersion(version), showAndroidPackages)}
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
  showDiagnostics: boolean,
  showAndroidPackages: boolean,
): AppShellNavItem[] {
  const items: AppShellNavItem[] = [
    { to: "/", label: t("nav.catalog"), icon: "catalog", exact: true },
    { to: "/library", label: t("nav.library"), icon: "library" },
    { to: "/scanner", label: t("nav.scanner"), icon: "scanner" },
    { to: "/import", label: t("nav.import"), icon: "import" },
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
  if (showDiagnostics) {
    items.push({
      to: "/diagnostics",
      label: t("nav.diagnostics"),
      icon: "diagnostics",
      group: "secondary",
      developerOnly: true,
    });
  }
  return items;
}

function ScannerRoute() {
  const navigate = scannerRoute.useNavigate();
  return (
    <ScannerPage
      gateway={tauriScannerGateway}
      onReviewInstallation={(installationId) =>
        navigate({
          to: "/library/$installationId",
          params: { installationId },
          search: { intent: undefined },
        })
      }
    />
  );
}

function LibraryRoute() {
  const navigate = libraryRoute.useNavigate();
  return (
    <LibraryPage
      gateway={tauriLibraryGateway}
      androidAppGateway={__DLA_TARGET_PLATFORM__ === "android" ? tauriAndroidAppGateway : undefined}
      catalogGateway={tauriCatalogGateway}
      onOpenReview={(installationId) =>
        navigate({
          to: "/library/$installationId",
          params: { installationId },
          search: { intent: undefined },
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
  );
}

function AndroidPackageRoute() {
  const { work } = androidPackageRoute.useSearch();
  const navigate = androidPackageRoute.useNavigate();
  return (
    <AndroidPackagePage
      gateway={tauriAndroidPackageGateway}
      associationGateway={tauriAndroidAppGateway}
      workCode={work}
      onOpenLibrary={() => navigate({ to: "/library" })}
    />
  );
}

function ImportRoute() {
  const navigate = importRoute.useNavigate();
  return (
    <CatalogImportPage
      gateway={tauriCatalogImportGateway}
      onOpenCatalog={() => navigate({ to: "/", search: catalogRouteSearch(defaultCatalogRouteState) })}
    />
  );
}

function LibraryReviewRoute() {
  const { installationId } = libraryReviewRoute.useParams();
  const { intent } = libraryReviewRoute.useSearch();
  const navigate = libraryReviewRoute.useNavigate();
  return (
    <LibraryReviewPage
      installationId={installationId}
      gateway={tauriLibraryGateway}
      onBack={() => navigate({ to: "/library" })}
      onOpenWork={(code) => navigate({ to: "/works/$code", params: { code } })}
      focusPreparation={intent === "install"}
    />
  );
}

function LibraryMediaRoute() {
  const { installationId } = libraryMediaRoute.useParams();
  const navigate = libraryMediaRoute.useNavigate();
  return (
    <MediaSessionPage
      installationId={installationId}
      gateway={tauriLibraryGateway}
      catalogGateway={tauriCatalogGateway}
      onBack={() => navigate({ to: "/library" })}
    />
  );
}

function CatalogRoute() {
  const filters = catalogRoute.useSearch();
  const navigate = catalogRoute.useNavigate();
  const updateRoute = useCallback((
    change: Partial<Pick<typeof filters, "sort" | "timeline" | "month" | "page">>,
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
  const back = () => {
    if (window.history.length > 1) window.history.back();
    else void navigate({ to: "/", search: catalogRouteSearch(defaultCatalogRouteState) });
  };

  return (
    <Suspense fallback={<DetailModuleLoading />}>
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
        onInstallAndroidApp={__DLA_TARGET_PLATFORM__ === "android"
          ? (work) => navigate({ to: "/android-packages", search: { work } })
          : undefined}
        onOpenExternal={openUrl}
      />
    </Suspense>
  );
}

function DetailModuleLoading() {
  const { t } = usePresentation();
  return (
    <main className="work-detail-loading" aria-live="polite" aria-label={t("detail.routeLoading")}>
      <div className="work-detail-loading-layout"><span /><span /></div>
      <div className="route-loading-overlay"><i /><strong>{t("detail.routeLoading")}</strong></div>
    </main>
  );
}
