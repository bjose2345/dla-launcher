import { invoke } from "@tauri-apps/api/core";

import { webkitGtkWebview } from "./webview";

/** Brings back a page WebKitGTK stopped drawing; the other webviews need none. */
export const recoverWebviewRendering: (() => Promise<void>) | null = webkitGtkWebview
  ? () => invoke("recover_webview_rendering")
  : null;
