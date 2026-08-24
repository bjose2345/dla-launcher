import react from "@vitejs/plugin-react";
import { defineConfig, loadEnv } from "vite";

import launcherPackage from "../../package.json" with { type: "json" };

export default defineConfig(({ mode }) => {
  const environment = loadEnv(mode, ".", "");
  const mobileHost = environment.TAURI_DEV_HOST;
  return {
    clearScreen: false,
    define: {
      __DLA_LAUNCHER_VERSION__: JSON.stringify(launcherPackage.version),
      __DLA_TARGET_PLATFORM__: JSON.stringify(environment.TAURI_ENV_PLATFORM ?? "desktop"),
    },
    plugins: [react()],
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
