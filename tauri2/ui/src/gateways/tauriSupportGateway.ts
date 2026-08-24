import type {
  FrontendFaultReport,
  SupportGateway,
  SupportSaveResult,
  SupportStatus,
} from "@dla-launcher/shared-ui/support";
import { invoke } from "@tauri-apps/api/core";

export const tauriSupportGateway: SupportGateway = {
  readStatus(): Promise<SupportStatus> {
    return invoke("read_support_status");
  },
  acknowledgeUncleanShutdown(): Promise<void> {
    return invoke("acknowledge_unclean_shutdown");
  },
  recordFrontendFault(report: FrontendFaultReport): Promise<void> {
    return invoke("record_frontend_fault", { report });
  },
  saveBundle(): Promise<SupportSaveResult> {
    return invoke("save_support_bundle");
  },
  openIssue(): Promise<void> {
    return invoke("open_support_issue");
  },
  openProject(): Promise<void> {
    return invoke("open_support_project");
  },
};
