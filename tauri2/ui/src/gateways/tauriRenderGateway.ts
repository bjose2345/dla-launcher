import { invoke } from "@tauri-apps/api/core";

/** Asks the shell to bring back a page that WebKitGTK stopped drawing. */
export function recoverWebviewRendering(): Promise<void> {
  return invoke("recover_webview_rendering");
}
