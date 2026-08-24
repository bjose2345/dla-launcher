import type { ProbeCheck } from "./types";

export type CheckSummary = {
  passed: number;
  failed: number;
  total: number;
};

export function summarizeChecks(checks: ProbeCheck[]): CheckSummary {
  return checks.reduce<CheckSummary>(
    (summary, check) => ({
      passed: summary.passed + Number(check.passed),
      failed: summary.failed + Number(!check.passed),
      total: summary.total + 1,
    }),
    { passed: 0, failed: 0, total: 0 },
  );
}
