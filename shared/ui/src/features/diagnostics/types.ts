export interface ProbeCheck {
  key: string;
  label: string;
  passed: boolean;
  detail: string;
}

export interface ProbeReport {
  passed: boolean;
  platform: string;
  databasePath: string;
  sqliteVersion: string;
  journalMode: string;
  completedAt: string;
  checks: ProbeCheck[];
}

export interface DiagnosticsGateway {
  runSQLiteProbe(): Promise<ProbeReport>;
}
