import type { DiagnosticsGateway, ProbeReport } from "@dla-launcher/shared-ui/diagnostics";
import type {
  SystemGateway,
  SystemReport,
  WindowGateway,
  WindowMetrics,
  WindowSize,
} from "@dla-launcher/shared-ui/preferences";
import { invoke } from "@tauri-apps/api/core";

export const tauriDiagnosticsGateway: DiagnosticsGateway & WindowGateway & SystemGateway = {
  runSQLiteProbe(): Promise<ProbeReport> {
    return invoke("run_sqlite_probe");
  },
  readSystemReport(): Promise<SystemReport> {
    return invoke("read_system_report");
  },
  readWindowMetrics(): Promise<WindowMetrics> {
    return invoke("read_window_metrics");
  },
  resizeWindow(size: WindowSize): Promise<WindowMetrics> {
    return invoke("resize_window", { size });
  },
  maximizeWindow(): Promise<WindowMetrics> {
    return invoke("maximize_window");
  },
};
