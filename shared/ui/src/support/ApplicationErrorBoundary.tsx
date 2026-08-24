import { Component, type ErrorInfo, type ReactNode } from "react";

import type { SupportGateway } from "./types";

export interface ApplicationErrorLabels {
  title: string;
  help: string;
  copy: string;
  copied: string;
  save: string;
  saving: string;
  saved: string;
  reload: string;
  actionFailed: string;
}

interface ApplicationErrorBoundaryProps {
  children: ReactNode;
  gateway: SupportGateway;
  labels: ApplicationErrorLabels;
}

interface ApplicationErrorBoundaryState {
  error: Error | null;
  componentStack: string;
  action: "idle" | "copied" | "saving" | "saved" | "failed";
}

export class ApplicationErrorBoundary extends Component<
  ApplicationErrorBoundaryProps,
  ApplicationErrorBoundaryState
> {
  private recording: Promise<void> = Promise.resolve();

  state: ApplicationErrorBoundaryState = {
    error: null,
    componentStack: "",
    action: "idle",
  };

  static getDerivedStateFromError(error: Error): Partial<ApplicationErrorBoundaryState> {
    return { error };
  }

  componentDidCatch(error: Error, information: ErrorInfo): void {
    this.setState({ componentStack: information.componentStack ?? "" });
    this.recording = this.props.gateway.recordFrontendFault({
      kind: "frontendRender",
      message: error.message || error.name,
      stack: error.stack ?? "",
      componentStack: information.componentStack ?? "",
    });
    void this.recording.catch(() => undefined);
  }

  render(): ReactNode {
    const { error, action } = this.state;
    if (!error) return this.props.children;
    const { labels } = this.props;
    return (
      <main className="support-fallback" role="alert">
        <div className="support-fallback-panel">
          <span className="support-fallback-code">RECOVERY / UI</span>
          <h1>{labels.title}</h1>
          <p>{labels.help}</p>
          <div className="support-fallback-actions">
            <button type="button" onClick={() => void this.copyDetails()}>
              {action === "copied" ? labels.copied : labels.copy}
            </button>
            <button type="button" disabled={action === "saving"} onClick={() => void this.save()}>
              {action === "saving" ? labels.saving : action === "saved" ? labels.saved : labels.save}
            </button>
            <button className="is-primary" type="button" onClick={() => window.location.reload()}>
              {labels.reload}
            </button>
          </div>
          {action === "failed" ? <p className="support-fallback-error">{labels.actionFailed}</p> : null}
          <details>
            <summary>{labels.copy}</summary>
            <pre>{this.fallbackText()}</pre>
          </details>
        </div>
      </main>
    );
  }

  private fallbackText(): string {
    const error = this.state.error;
    return [error?.name, error?.message, error?.stack, this.state.componentStack]
      .filter(Boolean)
      .join("\n");
  }

  private async copyDetails(): Promise<void> {
    try {
      await this.recording;
      const status = await this.props.gateway.readStatus();
      await navigator.clipboard.writeText(status.summary);
      this.setState({ action: "copied" });
    } catch {
      try {
        await navigator.clipboard.writeText(redactFallbackText(this.fallbackText()));
        this.setState({ action: "copied" });
      } catch {
        this.setState({ action: "failed" });
      }
    }
  }

  private async save(): Promise<void> {
    this.setState({ action: "saving" });
    try {
      await this.recording;
      const result = await this.props.gateway.saveBundle();
      this.setState({ action: result.outcome === "saved" ? "saved" : "idle" });
    } catch {
      this.setState({ action: "failed" });
    }
  }
}

function redactFallbackText(value: string): string {
  return value
    .replace(/file:\/\/\/[^\s)]+/giu, "<path>")
    .replace(/(?:[a-z]:[\\/]|\/)[^\s)]+/giu, "<path>");
}
