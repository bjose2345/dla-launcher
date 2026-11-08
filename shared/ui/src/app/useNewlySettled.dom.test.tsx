// @vitest-environment jsdom

import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { useNewlySettled } from "./useNewlySettled";

interface Job {
  id: string;
  status: "queued" | "running" | "done" | "failed";
}

const SETTLED = ["done", "failed"] as const;

describe("useNewlySettled", () => {
  it("announces a job that settled since the list was last seen, once, and not older ones", () => {
    const first: Job[] = [{ id: "old", status: "done" }, { id: "new", status: "running" }];
    const view = renderHook(
      ({ jobs }: { jobs: Job[] | undefined }) => useNewlySettled(jobs, (job) => job.id, (job) => job.status, SETTLED),
      { initialProps: { jobs: undefined as Job[] | undefined } },
    );
    expect(view.result.current[0]).toBeNull();

    view.rerender({ jobs: first });
    expect(view.result.current[0]).toBeNull();

    view.rerender({ jobs: [{ id: "old", status: "done" }, { id: "new", status: "failed" }] });
    expect(view.result.current[0]).toEqual({ id: "new", status: "failed" });

    act(() => view.result.current[1]());
    expect(view.result.current[0]).toBeNull();

    view.rerender({ jobs: [{ id: "old", status: "done" }, { id: "new", status: "failed" }, { id: "later", status: "done" }] });
    expect(view.result.current[0]).toEqual({ id: "later", status: "done" });
  });
});
