import type {
  ScanIssuePage,
  ScanProgress,
  ScanResultPage,
  ScanRootPreference,
  ScanSessionView,
  ScannerGateway,
  SelectedScanRoot,
} from "@dla-launcher/shared-ui/scanner";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export const tauriScannerGateway: ScannerGateway = {
  readRootPreference(): Promise<ScanRootPreference> {
    return invoke("read_scan_root_preference");
  },
  selectPreferredRoot(): Promise<ScanRootPreference | null> {
    return invoke("select_preferred_scan_root");
  },
  resetPreferredRoot(): Promise<ScanRootPreference> {
    return invoke("reset_scan_root_preference");
  },
  preparePreferredRoot(): Promise<SelectedScanRoot> {
    return invoke("prepare_preferred_scan_root");
  },
  selectRoot(): Promise<SelectedScanRoot | null> {
    return invoke("select_scan_root");
  },
  start(accessHandle: string): Promise<ScanSessionView> {
    return invoke("start_library_scan", { accessHandle });
  },
  cancel(sessionId: string): Promise<boolean> {
    return invoke("cancel_library_scan", { sessionId });
  },
  readLatest(): Promise<ScanSessionView | null> {
    return invoke("read_latest_library_scan");
  },
  browseResults(request): Promise<ScanResultPage> {
    return invoke("browse_library_scan_results", { request });
  },
  browseIssues(request): Promise<ScanIssuePage> {
    return invoke("browse_library_scan_issues", { request });
  },
  createInstallation(sessionId: string, selectedResultId: string): Promise<{ id: string }> {
    return invoke("create_installation_from_scan", { sessionId, selectedResultId });
  },
  subscribeProgress(listener): Promise<() => void> {
    return listen<ScanProgress>("scanner-progress", (event) => listener(event.payload));
  },
};
