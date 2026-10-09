import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";

import { translate } from "../../preferences";
import { jobProgressFraction, jobStatusText, problemText, remainingText } from "./deliveryText";
import { deliveryFailure, latestJob, receivedBytes, type DeliveryJob, type DeliveryView } from "./types";
import { acceptDeliveryView, DELIVERY_QUERY_KEY } from "./useDelivery";

const t = (key: Parameters<typeof translate>[1], values?: Record<string, string | number>) =>
  translate("en-US", key, values);

function job(id: string, status: DeliveryJob["status"], extra: Partial<DeliveryJob> = {}): DeliveryJob {
  return {
    id,
    workCode: "RJ01055431",
    releaseDate: "2025-11-07",
    version: "",
    updateDate: "",
    archives: [
      { fileName: "RJ01055431.part1.exe", size: 600, sha256: "a".repeat(64), remotePath: null, complete: true },
      { fileName: "RJ01055431.part2.rar", size: 400, sha256: "b".repeat(64), remotePath: null, complete: false },
    ],
    installFolder: "/games",
    status,
    receivedBytes: 600,
    problem: null,
    detail: null,
    installationId: null,
    createdAt: "2026-10-07T20:00:00Z",
    updatedAt: "2026-10-07T20:00:00Z",
    ...extra,
  };
}

describe("Install from DLA", () => {
  it("reads refusals from Rust and anything else as unavailable", () => {
    expect(deliveryFailure({ problem: "allowance", detail: "spent" })).toEqual({ problem: "allowance", detail: "spent" });
    expect(deliveryFailure({ problem: "made_up", detail: "x" }).problem).toBe("unavailable");
    expect(deliveryFailure(new Error("boom"))).toEqual({ problem: "unavailable", detail: "boom" });
  });

  it("follows the newest job of a work, whatever the code's case", () => {
    const view: DeliveryView = {
      revision: 3,
      available: true,
      account: null,
      installFolder: null,
      progress: null,
      jobs: [
        job("old", "cancelled", { createdAt: "2026-10-07T19:00:00Z" }),
        job("new", "paused", { workCode: "rj01055431" }),
        job("other", "queued", { workCode: "RJ999" }),
      ],
    };
    expect(latestJob(view, "RJ01055431")?.id).toBe("new");
    expect(latestJob(view, "RJ0000")).toBeNull();
    expect(latestJob(undefined, "RJ01055431")).toBeNull();
  });

  it("keeps the newest view when a reply arrives after a later change", () => {
    const client = new QueryClient();
    const view = (revision: number, status: DeliveryJob["status"]): DeliveryView => ({
      revision, available: true, account: "demo", installFolder: "/games", progress: null, jobs: [job("1", status)],
    });
    acceptDeliveryView(client, view(2, "installed"));
    acceptDeliveryView(client, view(1, "queued"));
    expect(client.getQueryData<DeliveryView>(DELIVERY_QUERY_KEY)?.jobs[0]?.status).toBe("installed");
    acceptDeliveryView(client, view(2, "needs_review"));
    expect(client.getQueryData<DeliveryView>(DELIVERY_QUERY_KEY)?.jobs[0]?.status).toBe("needs_review");
  });

  it("shows live progress for the running job only", () => {
    const running = job("running", "downloading");
    const progress = { jobId: "running", workCode: "RJ01055431", phase: "receiving" as const, receivedBytes: 750, totalBytes: 1000 };
    expect(receivedBytes(running, progress)).toBe(750);
    expect(receivedBytes(job("other", "paused"), progress)).toBe(600);
    expect(jobProgressFraction(running, progress)).toBe(0.75);
    expect(jobProgressFraction(job("x", "installing"), null)).toBe(1);
    expect(jobStatusText(t, "en-US", running, progress)).toBe("Downloading 75% · 750 B of 1,000 B");
    expect(jobStatusText(t, "en-US", running, { ...progress, phase: "locating" })).toBe("Finding the files on DLA…");
    expect(jobStatusText(t, "en-US", job("p", "paused"), null)).toBe("Paused · 600 B of 1,000 B");
  });

  it("shows unpacking progress and the time left", () => {
    const installing = job("i", "installing");
    const unpacking = { jobId: "i", workCode: "RJ01055431", phase: "unpacking" as const, receivedBytes: 300, totalBytes: 1200 };
    expect(jobStatusText(t, "en-US", installing, unpacking)).toBe("Unpacking 25%");
    expect(jobProgressFraction(installing, unpacking)).toBe(0.25);
    expect(jobStatusText(t, "en-US", installing, null)).toBe("Installing…");
    expect(remainingText(t, 1000, 0)).toBeNull();
    expect(remainingText(t, 0, 50)).toBeNull();
    expect(remainingText(t, 30 * 1024, 1024)).toBe("less than a minute left");
    expect(remainingText(t, 12 * 60 * 1024, 1024)).toBe("about 12 min left");
    expect(remainingText(t, 125 * 60 * 1024, 1024)).toBe("about 2 h 5 min left");
  });

  it("phrases every problem in every state", () => {
    expect(jobStatusText(t, "en-US", job("f", "failed", { problem: "busy" }), null))
      .toBe("Stopped: This account is already downloading on another device.");
    for (const problem of [
      "sign_in_required", "credentials", "too_many_attempts", "unreachable", "not_in_archive", "allowance",
      "busy", "disk_space", "corrupt", "install_folder", "install", "unavailable",
    ] as const) {
      expect(problemText(t, problem)).not.toContain("delivery.problem");
    }
    for (const status of ["queued", "installing", "installed", "needs_review", "cancelled"] as const) {
      expect(jobStatusText(t, "en-US", job("s", status), null)).not.toContain("delivery.status");
    }
  });
});
