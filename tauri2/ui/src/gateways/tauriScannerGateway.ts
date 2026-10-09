import type {
  ScanAddResponse,
  ScanFindingList,
  ScanInstallView,
  ScanIssuePage,
  ScanProgress,
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
  listFindings(sessionId): Promise<ScanFindingList> {
    return invoke("list_scan_findings", { sessionId });
  },
  setFindingIgnored(rootId, key, ignored): Promise<void> {
    return invoke("set_scan_finding_ignored", { rootId, key, ignored });
  },
  addFindings(sessionId, findings): Promise<ScanAddResponse> {
    return invoke("add_scan_findings", { request: { sessionId, findings } });
  },
  browseIssues(request): Promise<ScanIssuePage> {
    return invoke("browse_library_scan_issues", { request });
  },
  subscribeProgress(listener): Promise<() => void> {
    return listen<ScanProgress>("scanner-progress", (event) => listener(event.payload));
  },
  readInstalls(): Promise<ScanInstallView> {
    return invoke("read_scan_installs");
  },
  subscribeInstalls(listener): Promise<() => void> {
    return listen<ScanInstallView>("scan-installs-changed", (event) => listener(event.payload));
  },
  selectInstallFolder(): Promise<ScanInstallView | null> {
    return invoke("select_install_folder");
  },
  installArchive(installationId): Promise<ScanInstallView> {
    return invoke("install_library_archive", { installationId });
  },
  cancelInstall(installationId): Promise<ScanInstallView> {
    return invoke("cancel_scan_install", { installationId });
  },
};
