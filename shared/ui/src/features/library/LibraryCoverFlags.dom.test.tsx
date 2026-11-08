// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { PresentationProvider } from "../../preferences/PresentationProvider";
import { TranslatedWorksProvider } from "../translation/TranslatedWorks";
import type { TranslationGateway } from "../translation/types";
import { LibraryCoverFlags } from "./LibraryCoverFlags";

afterEach(() => {
  cleanup();
});

describe("LibraryCoverFlags", () => {
  it("puts EN beside what the work is when the Archive has a translation", async () => {
    const gateway = {
      translatedWorks: vi.fn().mockResolvedValue({ RJ112559: "2026-11-03T10:50:49Z" }),
    } as unknown as TranslationGateway;
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { container } = render(
      <QueryClientProvider client={client}>
        <PresentationProvider>
          <TranslatedWorksProvider gateway={gateway}>
            <LibraryCoverFlags kind="apps" code="RJ112559" />
            <LibraryCoverFlags kind="apps" code="RJ01" />
          </TranslatedWorksProvider>
        </PresentationProvider>
      </QueryClientProvider>,
    );

    await screen.findByText("EN");
    const [translated, untranslated] = Array.from(container.querySelectorAll(".library-cover-flags"));
    expect(Array.from(translated!.children, (flag) => flag.textContent)).toEqual(["Apps", "EN"]);
    expect(untranslated!.children).toHaveLength(1);
  });
});
