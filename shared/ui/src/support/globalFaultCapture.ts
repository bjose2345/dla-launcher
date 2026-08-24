import type { FrontendFaultReport, SupportGateway } from "./types";

const MAX_TEXT_LENGTH = 64 * 1024;
const MAX_SEEN_FAILURES = 32;

export function installGlobalFaultCapture(gateway: SupportGateway): () => void {
  const seen = new Set<string>();
  const record = (report: FrontendFaultReport) => {
    const key = `${report.kind}:${report.message}:${report.stack}`.slice(0, 2048);
    if (seen.has(key)) return;
    seen.add(key);
    if (seen.size > MAX_SEEN_FAILURES) {
      const oldest = seen.values().next().value;
      if (oldest) seen.delete(oldest);
    }
    void gateway.recordFrontendFault({
      ...report,
      message: bound(report.message),
      stack: bound(report.stack),
      componentStack: bound(report.componentStack),
    }).catch(() => undefined);
  };
  const onError = (event: ErrorEvent) => {
    const error = toError(event.error ?? event.message);
    record({
      kind: "frontendError",
      message: error.message,
      stack: error.stack ?? "",
      componentStack: "",
    });
  };
  const onUnhandledRejection = (event: PromiseRejectionEvent) => {
    const error = toError(event.reason);
    record({
      kind: "unhandledRejection",
      message: error.message,
      stack: error.stack ?? "",
      componentStack: "",
    });
  };
  window.addEventListener("error", onError);
  window.addEventListener("unhandledrejection", onUnhandledRejection);
  return () => {
    window.removeEventListener("error", onError);
    window.removeEventListener("unhandledrejection", onUnhandledRejection);
  };
}

function toError(value: unknown): Error {
  if (value instanceof Error) return value;
  if (typeof value === "string") return new Error(value);
  try {
    return new Error(JSON.stringify(value));
  } catch {
    return new Error("Unknown frontend failure");
  }
}

function bound(value: string): string {
  return value.length > MAX_TEXT_LENGTH
    ? `${value.slice(0, MAX_TEXT_LENGTH)}\n<truncated>`
    : value;
}
