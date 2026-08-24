import type { ReadOnlyDeepLinkGateway } from "@dla-launcher/shared-ui/app";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export const tauriDeepLinkGateway: ReadOnlyDeepLinkGateway = {
  readCurrent(): Promise<readonly string[]> {
    return invoke<string[]>("read_current_read_only_deep_links");
  },
  subscribe(listener): Promise<() => void> {
    return listen<string[]>("read-only-deep-link", (event) => listener(event.payload));
  },
};
