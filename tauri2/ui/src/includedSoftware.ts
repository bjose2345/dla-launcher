import type { IncludedSoftware } from "@dla-launcher/shared-ui/about";

/**
 * The programs the desktop packages install beside the launcher. Versions
 * follow tauri2/scripts/fetch-7zip.sh and fetch-cloudflared.sh.
 */
export const includedSoftware: readonly IncludedSoftware[] = [
  {
    name: "7-Zip",
    version: "26.04",
    author: "Igor Pavlov",
    licenses: "LGPL-2.1-or-later, BSD-3-Clause, BSD-2-Clause, unRAR",
  },
  {
    name: "cloudflared",
    version: "2026.9.3",
    author: "Cloudflare, Inc.",
    licenses: "Apache-2.0",
  },
];
