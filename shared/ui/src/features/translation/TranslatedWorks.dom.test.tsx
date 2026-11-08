// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { PresentationProvider } from "../../preferences/PresentationProvider";
import { ArchiveTranslationBadge, TranslatedWorksProvider } from "./TranslatedWorks";
import type { TranslationGateway } from "./types";

afterEach(() => {
  cleanup();
});

describe("ArchiveTranslationBadge", () => {
  it("marks only the works the Archive has a translation of", async () => {
    const gateway = {
      translatedWorks: vi.fn().mockResolvedValue({ RJ112559: "2026-11-03T10:36:17Z" }),
    } as unknown as TranslationGateway;
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <PresentationProvider>
          <TranslatedWorksProvider gateway={gateway}>
            <ArchiveTranslationBadge code="RJ112559" className="translated" />
            <ArchiveTranslationBadge code="RJ01" className="untranslated" />
            <ArchiveTranslationBadge code={null} className="unknown" />
          </TranslatedWorksProvider>
        </PresentationProvider>
      </QueryClientProvider>,
    );

    const badge = await screen.findByLabelText("Translation from the Archive (Nov 3, 2026)");
    expect(badge.className).toBe("translated");
    expect(badge.textContent).toBe("EN");
    expect(document.querySelector(".untranslated")).toBeNull();
    expect(document.querySelector(".unknown")).toBeNull();
    expect(gateway.translatedWorks).toHaveBeenCalledTimes(1);
  });
});
