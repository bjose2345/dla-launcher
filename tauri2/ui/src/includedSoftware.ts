import type { IncludedSoftware } from "@dla-launcher/shared-ui/about";

/**
 * The programs and data the desktop packages install with the launcher.
 * Versions follow tauri2/scripts/fetch-7zip.sh and fetch-cloudflared.sh; the
 * name list's is the day its JMnedict was downloaded (build-name-list.py).
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
    version: "2026.10.6",
    author: "Cloudflare, Inc.",
    licenses: "Apache-2.0",
  },
  {
    name: "JMnedict (trimmed name list)",
    version: "2026-10-30",
    author: "Electronic Dictionary Research and Development Group",
    licenses: "CC BY-SA 4.0",
  },
];
