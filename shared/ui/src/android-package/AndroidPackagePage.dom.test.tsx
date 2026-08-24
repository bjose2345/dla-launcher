// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";

import { PresentationProvider } from "../preferences/PresentationProvider";
import { AndroidPackagePage } from "./AndroidPackagePage";
import type {
  AndroidAppGateway,
  AndroidAppView,
  AndroidPackageGateway,
  AndroidPackageInspection,
  AndroidPackageState,
} from "./types";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("AndroidPackagePage", () => {
  it("explains that the capability is absent outside Android", async () => {
    renderPage(gateway(unavailableState));

    expect(await screen.findByRole("heading", { name: "APK installation is unavailable here" }))
      .toBeTruthy();
    expect(screen.queryByRole("button", { name: "Choose APK" })).toBeNull();
  });

  it("uses the system selection result and waits for Android confirmation", async () => {
    const selected = state({ inspection });
    const awaiting = state({
      inspection,
      installStatus: {
        operationId: "87654321-4321-4321-4321-cba987654321",
        selectionId: inspection.selectionId,
        state: "awaiting_user_confirmation",
        technicalDetail: null,
      },
    });
    const mock = gateway(emptyState, {
      selectAndInspect: vi.fn().mockResolvedValue(selected),
      requestInstall: vi.fn().mockResolvedValue(awaiting),
    });
    renderPage(mock);

    fireEvent.click(await screen.findByRole("button", { name: "Choose APK" }));
    expect(await screen.findByRole("heading", { name: "Fixture app" })).toBeTruthy();
    expect(screen.getByText("fixture.apk")).toBeTruthy();
    expect(screen.getByText("Signing certificate found")).toBeTruthy();
    expect(document.body.textContent).not.toContain(inspection.selectionId);

    fireEvent.click(screen.getByRole("button", { name: "Continue to Android" }));
    expect(await screen.findByText("Waiting for your confirmation")).toBeTruthy();
    expect(screen.getByText("Use Android's system screen to confirm or cancel.")).toBeTruthy();
    expect(mock.requestInstall).toHaveBeenCalledOnce();
  });

  it("refuses a package blocked by native inspection", async () => {
    const blocked = state({
      inspection: {
        ...inspection,
        installable: false,
        blockReason: "split_package",
      },
    });
    renderPage(gateway(blocked));

    expect(await screen.findByText("This is part of a split APK set, which is not supported yet."))
      .toBeTruthy();
    expect((screen.getByRole("button", { name: "Continue to Android" }) as HTMLButtonElement).disabled)
      .toBe(true);
  });

  it("opens only Android's app-specific approval settings", async () => {
    const approval = state({
      capability: { status: "approval_required", deviceSdk: 36 },
      inspection,
    });
    const ready = state({ inspection });
    const mock = gateway(approval, {
      openSourceApproval: vi.fn().mockResolvedValue(ready),
    });
    renderPage(mock);

    fireEvent.click(await screen.findByRole("button", { name: "Open Android settings" }));
    await waitFor(() => expect(mock.openSourceApproval).toHaveBeenCalledOnce());
    expect(screen.queryByText("Allow installs from DLA Launcher")).toBeNull();
  });

  it("offers recovery when the initial native state cannot be read", async () => {
    const mock = gateway(emptyState);
    vi.mocked(mock.readState)
      .mockRejectedValueOnce(new Error("bridge unavailable"))
      .mockResolvedValueOnce(emptyState);
    renderPage(mock);

    expect((await screen.findByRole("alert")).textContent).toContain("bridge unavailable");
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByRole("heading", { name: "Choose an APK" })).toBeTruthy();
  });

  it("links only the completed installed selection to an explicit catalog work", async () => {
    const installed = state({
      inspection,
      installStatus: {
        operationId: "87654321-4321-4321-4321-cba987654321",
        selectionId: inspection.selectionId,
        state: "installed",
        technicalDetail: null,
      },
    });
    const associated = androidAppView();
    const associations: AndroidAppGateway = {
      list: vi.fn().mockResolvedValue([]),
      associateInstalled: vi.fn().mockResolvedValue(associated),
      launch: vi.fn().mockResolvedValue(associated),
      remove: vi.fn().mockResolvedValue(undefined),
    };
    renderPage(gateway(installed), associations, "RJ01326398");

    fireEvent.click(await screen.findByRole("button", { name: "Add to Library" }));
    await waitFor(() => expect(associations.associateInstalled).toHaveBeenCalledWith("RJ01326398"));
    expect(await screen.findByText("Added to your Library")).toBeTruthy();
  });
});

const inspection: AndroidPackageInspection = {
  selectionId: "12345678-1234-1234-1234-123456789abc",
  displayName: "fixture.apk",
  applicationLabel: "Fixture app",
  packageName: "org.dlaproject.fixture",
  versionName: "1.0",
  versionCode: "1",
  sizeBytes: 4096,
  sha256: "a".repeat(64),
  minimumSdk: 24,
  targetSdk: 36,
  signingCertificateSha256: ["b".repeat(64)],
  installable: true,
  blockReason: null,
};

const unavailableState: AndroidPackageState = {
  capability: { status: "unavailable", deviceSdk: null },
  inspection: null,
  installStatus: null,
};

const emptyState = state({});

function state(overrides: Partial<AndroidPackageState>): AndroidPackageState {
  return {
    capability: { status: "ready", deviceSdk: 36 },
    inspection: null,
    installStatus: null,
    ...overrides,
  };
}

function gateway(
  initial: AndroidPackageState,
  overrides: Partial<AndroidPackageGateway> = {},
): AndroidPackageGateway {
  return {
    readState: vi.fn().mockResolvedValue(initial),
    selectAndInspect: vi.fn().mockResolvedValue(initial),
    clearSelection: vi.fn().mockResolvedValue(emptyState),
    openSourceApproval: vi.fn().mockResolvedValue(initial),
    requestInstall: vi.fn().mockResolvedValue(initial),
    ...overrides,
  };
}

function renderPage(
  mock: AndroidPackageGateway,
  associationGateway?: AndroidAppGateway,
  workCode?: string,
) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <PresentationProvider>
        <AndroidPackagePage
          gateway={mock}
          associationGateway={associationGateway}
          workCode={workCode}
        />
      </PresentationProvider>
    </QueryClientProvider>,
  );
}

function androidAppView(): AndroidAppView {
  return {
    association: {
      id: "android-app-1234567890",
      workCode: "RJ01326398",
      packageName: inspection.packageName,
      applicationLabel: inspection.applicationLabel,
      expectedSigningCertificateSha256: inspection.signingCertificateSha256,
      associatedVersionName: inspection.versionName,
      associatedVersionCode: inspection.versionCode,
      associatedAt: "2026-08-22T12:00:00Z",
      updatedAt: "2026-08-22T12:00:00Z",
      lastLaunchedAt: null,
      launchCount: 0,
    },
    runtime: {
      state: "ready",
      applicationLabel: inspection.applicationLabel,
      versionName: inspection.versionName,
      versionCode: inspection.versionCode,
      technicalDetail: null,
    },
  };
}
