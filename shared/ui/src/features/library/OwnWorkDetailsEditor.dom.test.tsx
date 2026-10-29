// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { PresentationProvider } from "../../preferences/PresentationProvider";
import { OwnWorkDetailsEditor, type OwnWorkEditorGateway } from "./OwnWorkDetailsEditor";
import type { OwnWorkDetails, SaveOwnWorkDetailsRequest } from "./types";

afterEach(() => {
  cleanup();
});

describe("OwnWorkDetailsEditor", () => {
  it("saves a text field when it is left, and a kind or a tag at once", async () => {
    const { gateway, states } = renderEditor(details({ circle: "Old circle", tags: ["CG"] }));

    const circle = await screen.findByDisplayValue("Old circle");
    fireEvent.change(circle, { target: { value: "  New circle " } });
    fireEvent.blur(circle);
    await waitFor(() => expect(lastSave(gateway).circle).toBe("New circle"));
    expect(states).toContain("saving");
    await waitFor(() => expect(states.at(-1)).toBe("saved"));

    fireEvent.click(screen.getByRole("button", { name: "Manga" }));
    await waitFor(() => expect(lastSave(gateway).kind).toBe("manga"));

    const tags = screen.getByLabelText("Tags");
    fireEvent.change(tags, { target: { value: "Bonus" } });
    fireEvent.keyDown(tags, { key: "Enter" });
    await waitFor(() => expect(lastSave(gateway).tags).toEqual(["CG", "Bonus"]));

    fireEvent.click(screen.getByRole("button", { name: "Remove CG" }));
    await waitFor(() => expect(lastSave(gateway).tags).toEqual(["Bonus"]));
  });

  it("makes one of the work's own images the cover", async () => {
    const { gateway } = renderEditor(details());

    fireEvent.click(await screen.findByRole("button", { name: "Choose from this work" }));
    const tile = await screen.findByRole("button", { name: "Use 02.png as the cover" });
    expect(tile.querySelector("img")?.getAttribute("src")).toBe("thumb:installation-1:Set/02.png");
    fireEvent.click(tile);

    await waitFor(() => expect(gateway.importOwnWorkArtwork).toHaveBeenCalledWith("installation-1", "Set/02.png"));
    await waitFor(() => expect(lastSave(gateway).cover).toBe("art-Set/02.png"));
    expect(screen.queryByRole("button", { name: "Use 01.png as the cover" })).toBeNull();
    expect(screen.getByRole("img", { name: "Cover" }).getAttribute("src")).toBe("art:installation-1:art-Set/02.png");
  });

  it("takes a cover from a picture file and leaves it alone when the picker is cancelled", async () => {
    const { gateway } = renderEditor(details());
    gateway.importOwnWorkArtworkFile.mockResolvedValueOnce(null).mockResolvedValueOnce("art-file");

    fireEvent.click(await screen.findByRole("button", { name: "From a file…" }));
    await waitFor(() => expect(gateway.importOwnWorkArtworkFile).toHaveBeenCalledOnce());
    expect(gateway.saveOwnWorkDetails).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "From a file…" }));
    await waitFor(() => expect(lastSave(gateway).cover).toBe("art-file"));
  });

  it("collects sample images in the order they are picked and removes them", async () => {
    const { gateway } = renderEditor(details());

    fireEvent.click(await screen.findByRole("button", { name: "Pick images" }));
    fireEvent.click(await screen.findByRole("button", { name: "Add 03.png to the sample images" }));
    await waitFor(() => expect(lastSave(gateway).samples).toEqual(["art-Set/03.png"]));
    fireEvent.click(screen.getByRole("button", { name: "Add 01.png to the sample images" }));
    await waitFor(() => expect(lastSave(gateway).samples).toEqual(["art-Set/03.png", "art-Set/01.png"]));

    fireEvent.click(screen.getByRole("button", { name: "Remove sample image 1" }));
    await waitFor(() => expect(lastSave(gateway).samples).toEqual(["art-Set/01.png"]));
  });

  it("says when a change could not be saved", async () => {
    const { gateway, states } = renderEditor(details());
    gateway.saveOwnWorkDetails.mockRejectedValueOnce(new Error("disk full"));

    fireEvent.click(await screen.findByRole("button", { name: "Video" }));
    await waitFor(() => expect(states.at(-1)).toBe("failed"));
  });
});

function renderEditor(saved: OwnWorkDetails | null) {
  const states: string[] = [];
  const gateway = {
    readOwnWorkDetails: vi.fn().mockResolvedValue(saved),
    saveOwnWorkDetails: vi.fn(async (request: SaveOwnWorkDetailsRequest) => ({ ...request, updatedAt: "2026-10-28T01:00:00Z" })),
    listInstallationImages: vi.fn().mockResolvedValue([
      { relativePath: "Set/01.png", sizeBytes: 10 },
      { relativePath: "Set/02.png", sizeBytes: 10 },
      { relativePath: "Set/03.png", sizeBytes: 10 },
    ]),
    importOwnWorkArtwork: vi.fn(async (_installationId: string, relativePath: string) => `art-${relativePath}`),
    importOwnWorkArtworkFile: vi.fn<(installationId: string) => Promise<string | null>>(),
    ownWorkArtworkUrl: vi.fn((installationId: string, artworkId: string) => `art:${installationId}:${artworkId}`),
    installationImageUrl: vi.fn((installationId: string, relativePath: string) => `thumb:${installationId}:${relativePath}`),
  } satisfies OwnWorkEditorGateway;
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <PresentationProvider>
        <OwnWorkDetailsEditor installationId="installation-1" gateway={gateway} onSaveState={(state) => states.push(state)} />
      </PresentationProvider>
    </QueryClientProvider>,
  );
  return { gateway, states };
}

function lastSave(gateway: { saveOwnWorkDetails: { mock: { calls: unknown[][] } } }): SaveOwnWorkDetailsRequest {
  const calls = gateway.saveOwnWorkDetails.mock.calls;
  return calls[calls.length - 1]![0] as SaveOwnWorkDetailsRequest;
}

function details(patch: Partial<OwnWorkDetails> = {}): OwnWorkDetails {
  return {
    installationId: "installation-1",
    circle: null,
    code: null,
    releaseDate: null,
    kind: null,
    tags: [],
    description: null,
    cover: null,
    samples: [],
    updatedAt: "2026-10-28T00:00:00Z",
    ...patch,
  };
}
