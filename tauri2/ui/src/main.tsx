import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider } from "@tanstack/react-router";
import {
  installClipboardWriter,
  installReadOnlyDeepLinkNavigation,
  installRenderWatchdog,
  installRenderingCapabilities,
  installVideoCapabilities,
} from "@dla-launcher/shared-ui/app";
import { ReaderProvider, MediaPlaybackProvider } from "@dla-launcher/shared-ui/library/shell";
import { tauriCatalogGateway } from "./gateways/tauriCatalogGateway";
import { tauriLibraryGateway } from "./gateways/tauriLibraryGateway";
import { invoke } from "@tauri-apps/api/core";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { StrictMode, useEffect } from "react";
import { createRoot } from "react-dom/client";
import "@dla-launcher/shared-ui/styles.css";
import {
  initializePresentationPreferences,
  KeyBindingsProvider,
  loadLocaleCatalog,
  PresentationProvider,
  translate,
} from "@dla-launcher/shared-ui/preferences/shell";
import { ApplicationErrorBoundary, installGlobalFaultCapture } from "@dla-launcher/shared-ui/support/shell";

import { navigateReadOnlyDeepLink, router } from "./router";
import { tauriDeepLinkGateway } from "./gateways/tauriDeepLinkGateway";
import { recoverWebviewRendering } from "./gateways/tauriRenderGateway";
import { chromiumWebview } from "./gateways/webview";
import { tauriSupportGateway } from "./gateways/tauriSupportGateway";

const presentation = initializePresentationPreferences();
// The webview's own context menu (reload, inspect) stays a development tool.
if (!import.meta.env.DEV) {
  document.addEventListener("contextmenu", (event) => event.preventDefault());
}
installGlobalFaultCapture(tauriSupportGateway);
if (recoverWebviewRendering) installRenderWatchdog(recoverWebviewRendering);
installRenderingCapabilities({ cssBlur: chromiumWebview });
installVideoCapabilities({ nativeVideo: (await invoke<string>("read_video_backend")) === "native" });
installClipboardWriter(writeText);
await loadLocaleCatalog(presentation.locale);

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: false,
      staleTime: Number.POSITIVE_INFINITY,
    },
  },
});

const root = document.getElementById("root");

if (!root) {
  throw new Error("Application root was not found");
}

createRoot(root).render(
  <StrictMode>
    <ApplicationErrorBoundary
      gateway={tauriSupportGateway}
      labels={{
        title: translate(presentation.locale, "support.errorTitle"),
        help: translate(presentation.locale, "support.errorHelp"),
        copy: translate(presentation.locale, "support.copySummary"),
        copied: translate(presentation.locale, "support.copied"),
        save: translate(presentation.locale, "support.saveReport"),
        saving: translate(presentation.locale, "support.saving"),
        saved: translate(presentation.locale, "support.saved"),
        reload: translate(presentation.locale, "support.reload"),
        actionFailed: translate(presentation.locale, "support.actionFailed"),
      }}
    >
      <PresentationProvider>
        <KeyBindingsProvider>
          <QueryClientProvider client={queryClient}>
            <MediaPlaybackProvider gateway={tauriLibraryGateway}>
              <ReaderProvider gateway={tauriLibraryGateway} catalog={tauriCatalogGateway}>
                <RouterProvider router={router} />
                <RouterReady />
              </ReaderProvider>
            </MediaPlaybackProvider>
          </QueryClientProvider>
        </KeyBindingsProvider>
      </PresentationProvider>
    </ApplicationErrorBoundary>
  </StrictMode>,
);

let deepLinkNavigationStarted = false;

function RouterReady() {
  useEffect(() => {
    if (deepLinkNavigationStarted) return;
    deepLinkNavigationStarted = true;
    void installReadOnlyDeepLinkNavigation(
      tauriDeepLinkGateway,
      (target) => void navigateReadOnlyDeepLink(target),
    ).catch((cause: unknown) => {
      const error = cause instanceof Error ? cause : new Error(String(cause));
      return tauriSupportGateway
        .recordFrontendFault({
          kind: "startupFailure",
          message: `Deep-link initialization failed: ${error.message}`,
          stack: error.stack ?? "",
          componentStack: "",
        })
        .catch(() => undefined);
    });
  }, []);
  return null;
}
