import react from "@vitejs/plugin-react";
import { defineConfig, loadEnv, type Plugin } from "vite";

import launcherPackage from "../../package.json" with { type: "json" };

const sharedUiModule = /shared[\\/]ui[\\/]src[\\/]/;

// Modules the entry reaches through static imports alone, as opposed to the
// lazy routes. A code-splitting group's `test` only sees the module id, so the
// set is collected once the module graph is complete.
function startupModules(): { plugin: Plugin; has: (id: string) => boolean } {
  const ids = new Set<string>();
  return {
    has: (id) => ids.has(id),
    plugin: {
      name: "dla:startup-modules",
      apply: "build",
      buildStart() {
        ids.clear();
      },
      buildEnd() {
        const pending = [...this.getModuleIds()].filter((id) => this.getModuleInfo(id)?.isEntry);
        for (let id = pending.pop(); id !== undefined; id = pending.pop()) {
          if (ids.has(id)) continue;
          ids.add(id);
          pending.push(...(this.getModuleInfo(id)?.importedIds ?? []));
        }
      },
    },
  };
}

export default defineConfig(({ mode }) => {
  const environment = loadEnv(mode, ".", "");
  const mobileHost = environment.TAURI_DEV_HOST;
  const startup = startupModules();
  return {
    clearScreen: false,
    define: {
      __DLA_LAUNCHER_VERSION__: JSON.stringify(launcherPackage.version),
      __DLA_TARGET_PLATFORM__: JSON.stringify(environment.TAURI_ENV_PLATFORM ?? "desktop"),
      // "chromium" for the experimental Chromium build (tauri3/), whose webview
      // differs from the system one the Tauri 2 build runs on.
      __DLA_WEBVIEW__: JSON.stringify(environment.DLA_WEBVIEW ?? "system"),
    },
    plugins: [react(), startup.plugin],
    build: {
      rolldownOptions: {
        output: {
          // Shared icons and helpers otherwise become dozens of sub-kilobyte
          // chunks, each a separate request through the asset protocol. Small
          // helpers only lazy routes share get a chunk of their own, so that
          // startup does not carry them. Groups claim modules in declaration
          // order and take their dependencies along, so the startup group
          // comes first and spans every module: it must own a startup module
          // before the route group reaches it through a lazy helper.
          codeSplitting: {
            groups: [
              { name: "icons", test: /node_modules[\\/]lucide-react[\\/]/ },
              { name: "shared", test: (id) => startup.has(id), minShareCount: 2 },
              {
                name: "shared-routes",
                test: (id) => sharedUiModule.test(id) && !startup.has(id),
                minShareCount: 2,
                maxModuleSize: 8_000,
              },
            ],
          },
        },
      },
    },
    server: {
      host: mobileHost ?? "0.0.0.0",
      port: 1420,
      strictPort: true,
      hmr: mobileHost
        ? {
            protocol: "ws",
            host: mobileHost,
            port: 1421,
          }
        : undefined,
      watch: {
        ignored: ["**/src-tauri/**"],
      },
    },
  };
});
