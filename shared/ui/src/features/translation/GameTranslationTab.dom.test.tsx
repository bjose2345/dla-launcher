// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { DeliveryGateway } from "../delivery/types";
import { PresentationProvider } from "../../preferences/PresentationProvider";
import { GameTranslationTab } from "./GameTranslationTab";
import type { UntranslatableLanguage } from "./language";
import { TranslationSettingsSection } from "./TranslationSettingsSection";
import type {
  GameTranslationView,
  PreparationView,
  TranslationGateway,
  TranslationSettings,
} from "./types";

afterEach(() => {
  cleanup();
});

describe("GameTranslationTab", () => {
  it("offers to turn translation on for a game the launcher can translate", async () => {
    const gateway = translationGateway({ game: off() });
    vi.mocked(gateway.turnOn).mockResolvedValue(on(preparing()));
    renderTab(gateway);

    expect(await screen.findByText("Play this game in English")).toBeTruthy();
    expect(screen.getByText("RPG Maker MZ · Japanese")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Turn on translation" }));

    await waitFor(() => expect(gateway.turnOn).toHaveBeenCalledWith("installation-1"));
    expect(await screen.findByText("Preparing translation")).toBeTruthy();
  });

  it("asks for a translator first and explains how to set one up", async () => {
    const onOpenSettings = vi.fn();
    const gateway = translationGateway({ game: { ...off(), translatorReady: false } });
    renderTab(gateway, { onOpenSettings });

    expect(await screen.findByText("Set up a translator first")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Turn on translation" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Open translation settings" }));
    expect(onOpenSettings).toHaveBeenCalledOnce();

    fireEvent.click(screen.getByRole("button", { name: "How to set up a free translator" }));
    expect(screen.getByRole("dialog", { name: "Set up a free translator" })).toBeTruthy();
    expect(screen.getByText("ollama pull qwen3:4b-instruct")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "ollama.com" }));
    expect(gateway.openWebsite).toHaveBeenCalledWith("https://ollama.com/");
    fireEvent.click(screen.getByRole("tab", { name: "LM Studio" }));
    expect(screen.getByText("qwen/qwen3-4b-2507")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Done" }));
    expect(screen.queryByRole("dialog", { name: "Set up a free translator" })).toBeNull();
  });

  it("says when the engine is not supported yet", async () => {
    renderTab(translationGateway({
      game: { ...off(), engine: "unity_mono", engineLabel: "Unity", supported: false },
    }));

    expect(await screen.findByText("Translation for Unity games is coming")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Turn on translation" })).toBeNull();
  });

  it("says why a work in English, or not in Japanese, has nothing to translate", async () => {
    const english = renderTab(translationGateway({ game: off() }), { untranslatable: "includes_english" });

    expect(await screen.findByText("This work already includes English, so it doesn't need translating.")).toBeTruthy();
    expect(screen.queryByText("RPG Maker MZ · Japanese")).toBeNull();
    expect(screen.queryByRole("button", { name: "Turn on translation" })).toBeNull();
    english.unmount();

    renderTab(translationGateway({ game: off() }), { untranslatable: "not_japanese" });
    expect(await screen.findByText("The launcher translates Japanese works only, and this one isn't in Japanese."))
      .toBeTruthy();
  });

  it("keeps the controls of a work already translated, whatever the catalog says", async () => {
    renderTab(translationGateway({ game: on(preparing()) }), { untranslatable: "includes_english" });

    expect(await screen.findByText("Preparing translation")).toBeTruthy();
    expect(screen.queryByText("This work already includes English, so it doesn't need translating.")).toBeNull();
  });

  it("shows the progress of each group and pauses", async () => {
    const gateway = translationGateway({ game: on(preparing()) });
    vi.mocked(gateway.pause).mockResolvedValue(on({ ...preparing(), state: "paused", secondsLeft: null }));
    renderTab(gateway);

    expect(await screen.findByText("Preparing translation")).toBeTruthy();
    expect(screen.getByText("64%")).toBeTruthy();
    expect(screen.getByText("564 of 875 texts")).toBeTruthy();
    expect(screen.getByText("about 6 min left")).toBeTruthy();
    expect(screen.getByText("Names and descriptions")).toBeTruthy();
    expect(screen.getByText("70 of 121")).toBeTruthy();
    expect(screen.getByText("Local model · qwen3:4b-instruct")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Pause" }));
    expect(await screen.findByText("Preparation paused")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Resume" })).toBeTruthy();
  });

  it("follows the progress the launcher reports", async () => {
    let report: (view: GameTranslationView) => void = () => undefined;
    const gateway = translationGateway({ game: on(preparing()) });
    vi.mocked(gateway.subscribe).mockImplementation((listener) => {
      report = listener;
      return () => undefined;
    });
    renderTab(gateway);
    expect(await screen.findByText("564 of 875 texts")).toBeTruthy();

    act(() => report(on({ ...preparing(), done: 600 })));
    expect(await screen.findByText("600 of 875 texts")).toBeTruthy();
    // Another game's progress is not this one's.
    await act(async () => report({ ...on({ ...preparing(), done: 700 }), installationId: "other" }));
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(screen.getByText("600 of 875 texts")).toBeTruthy();
  });

  it("shows a ready game with play and the date it was prepared", async () => {
    const onPlay = vi.fn();
    const gateway = translationGateway({ game: on(ready()) });
    renderTab(gateway, { onPlay });

    expect(await screen.findByText("Translation ready")).toBeTruthy();
    expect(screen.getByText("875 texts · prepared on Oct 29, 2026 with qwen3:4b-instruct")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Play" }));
    expect(onPlay).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("button", { name: "Check for new lines" }));
    await waitFor(() => expect(gateway.prepare).toHaveBeenCalledWith("installation-1"));
  });

  it("translates a ready game again after a confirmation", async () => {
    const gateway = translationGateway({ game: on(ready()) });
    renderTab(gateway);

    fireEvent.click(await screen.findByRole("button", { name: "Translate again…" }));
    expect(screen.getByRole("alertdialog", {
      name: /^Translate the lines your translator made again\?/,
    })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(gateway.translateAgain).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Translate again…" }));
    fireEvent.click(screen.getByRole("button", { name: "Translate again" }));

    await waitFor(() => expect(gateway.translateAgain).toHaveBeenCalledWith("installation-1"));
    expect(await screen.findByRole("button", { name: "Pause" })).toBeTruthy();
  });

  it("switches the translation off in the game and removes it after a confirmation", async () => {
    const gateway = translationGateway({ game: on(ready()) });
    vi.mocked(gateway.show).mockResolvedValue({ ...on(ready()), shown: false });
    renderTab(gateway);

    const shown = await screen.findByRole("switch", { name: /Show the translation in the game/ });
    expect(shown.getAttribute("aria-checked")).toBe("true");
    fireEvent.click(shown);
    await waitFor(() => expect(gateway.show).toHaveBeenCalledWith("installation-1", false));

    fireEvent.click(screen.getByRole("switch", { name: /Prepare only while I'm not playing/ }));
    await waitFor(() => expect(gateway.saveSettings).toHaveBeenCalledWith({
      translator: settings().translator,
      prepareOnlyWhenIdle: true,
    }));

    fireEvent.click(screen.getByRole("button", { name: "Remove translation…" }));
    vi.mocked(gateway.readGame).mockResolvedValue(off());
    fireEvent.click(screen.getByRole("button", { name: "Remove translation" }));
    await waitFor(() => expect(gateway.remove).toHaveBeenCalledWith("installation-1"));
    expect(await screen.findByText("Play this game in English")).toBeTruthy();
  });

  it("offers the Archive's translation and shows the one in use", async () => {
    const game = on(ready());
    const gateway = translationGateway({ game });
    // Packs are made at a UTC time; ones made before that carry a day alone.
    const pack = { lines: 1551, made: "2026-10-30T12:00:00Z" };
    const installed = { lines: 1551, made: "2026-10-29" };
    vi.mocked(gateway.checkPack).mockResolvedValue({ ...game, pack: { available: pack, installed: null, access: null } });
    vi.mocked(gateway.downloadPack).mockResolvedValue({ ...game, pack: { available: null, installed, access: null } });
    renderTab(gateway);

    expect(await screen.findByText("A better translation of this game is available from the Archive.")).toBeTruthy();
    expect(screen.getByText("1,551 lines · made on Oct 30, 2026")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Download the translation" }));

    await waitFor(() => expect(gateway.downloadPack).toHaveBeenCalledWith("installation-1"));
    expect(await screen.findByText("Using the translation from the Archive")).toBeTruthy();
    expect(screen.getByText("1,551 lines · made on Oct 29, 2026")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Download the translation" })).toBeNull();
    expect(gateway.turnOn).not.toHaveBeenCalled();
  });

  it("offers the Archive's translation before translation is on, and turns it on to use it", async () => {
    const game = off();
    const gateway = translationGateway({ game: { ...game, translatorReady: false } });
    const pack = { lines: 14174, made: "2026-11-03T10:36:17Z" };
    vi.mocked(gateway.checkPack).mockResolvedValue({
      ...game,
      translatorReady: false,
      pack: { available: pack, installed: null, access: null },
    });
    vi.mocked(gateway.turnOn).mockResolvedValue(on(preparing()));
    vi.mocked(gateway.downloadPack).mockResolvedValue({
      ...on(ready()),
      pack: { available: null, installed: pack, access: null },
    });
    renderTab(gateway);

    expect(await screen.findByText("A better translation of this game is available from the Archive.")).toBeTruthy();
    expect(screen.getByText(/Works without a translator of your own/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Download the translation" }));

    await waitFor(() => expect(gateway.downloadPack).toHaveBeenCalledWith("installation-1"));
    expect(gateway.turnOn).toHaveBeenCalledWith("installation-1");
    const [turnedOn] = vi.mocked(gateway.turnOn).mock.invocationCallOrder;
    const [downloaded] = vi.mocked(gateway.downloadPack).mock.invocationCallOrder;
    expect(turnedOn).toBeLessThan(downloaded!);
    expect(await screen.findByText("Using the translation from the Archive")).toBeTruthy();
  });

  it("tells a member without Elite the Archive has one and where the plans are", async () => {
    const game = off();
    const gateway = translationGateway({ game });
    const onOpenExternal = vi.fn().mockResolvedValue(undefined);
    vi.mocked(gateway.checkPack).mockResolvedValue({
      ...game,
      pack: { available: null, installed: null, access: "for_elite" },
    });
    renderTab(gateway, { onOpenExternal });

    expect(await screen.findByText(
      "A better translation of this game is available from the Archive for Elite members.",
    )).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Download the translation" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "See the patron plans" }));
    expect(onOpenExternal).toHaveBeenCalledWith(expect.stringMatching(/\/patron\/plans$/));
  });

  it("tells a signed-out player the Archive's translations are for Elite members", async () => {
    const game = on(ready());
    const gateway = translationGateway({ game });
    vi.mocked(gateway.checkPack).mockResolvedValue({
      ...game,
      pack: { available: null, installed: null, access: "sign_in" },
    });
    const onOpenExternal = vi.fn().mockResolvedValue(undefined);
    const view = renderTab(gateway, { onOpenExternal });

    expect(await screen.findByText(/translations of games are for Elite members/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Sign in" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "See the patron plans" }));
    expect(onOpenExternal).toHaveBeenCalledWith(expect.stringMatching(/\/patron\/plans$/));
    view.unmount();

    renderTab(gateway, { deliveryGateway: {} as DeliveryGateway });
    expect(await screen.findByRole("button", { name: "Sign in" })).toBeTruthy();
  });

  it("says why preparation stopped and tries again", async () => {
    const gateway = translationGateway({
      game: on({
        ...preparing(),
        state: "failed",
        secondsLeft: null,
        problem: "translator_unreachable",
        error: "the translator could not be reached: io: Peer disconnected",
      }),
    });
    renderTab(gateway);

    expect(await screen.findByText("Preparation stopped")).toBeTruthy();
    expect(screen.getByText("The translator stopped answering. Check that it's running, then press Try again.")).toBeTruthy();
    expect(screen.getByText("Technical detail: the translator could not be reached: io: Peer disconnected")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(gateway.prepare).toHaveBeenCalledOnce());
  });
});

describe("TranslationSettingsSection", () => {
  it("tests the local model in the fields and keeps it when it works", async () => {
    const gateway = translationGateway({ settings: { translator: null, deeplKeySaved: false, prepareOnlyWhenIdle: false } });
    vi.mocked(gateway.testTranslator).mockResolvedValue({
      source: "オープニングを飛ばしますか？",
      english: "Skip the opening?",
      seconds: 2.94,
    });
    renderSettings(gateway);

    const address = await screen.findByLabelText("Address");
    expect((address as HTMLInputElement).value).toBe("http://127.0.0.1:11434/v1");
    fireEvent.change(screen.getByLabelText("Model"), { target: { value: "qwen3:4b" } });
    fireEvent.click(screen.getByRole("button", { name: "Test translator" }));

    expect(await screen.findByText("Works: オープニングを飛ばしますか？ → “Skip the opening?” · 2.9 s")).toBeTruthy();
    const local = { kind: "localModel", address: "http://127.0.0.1:11434/v1", model: "qwen3:4b" };
    expect(gateway.testTranslator).toHaveBeenCalledWith(local, undefined);
    await waitFor(() => expect(gateway.saveSettings).toHaveBeenCalledWith({ translator: local, deeplKey: undefined }));
  });

  it("keeps a DeepL key only in the launcher and says when one is saved", async () => {
    const gateway = translationGateway({
      settings: { translator: { kind: "deepl", freePlan: true }, deeplKeySaved: true, prepareOnlyWhenIdle: false },
    });
    vi.mocked(gateway.testTranslator).mockRejectedValue("the translator refused the request: HTTP 403");
    renderSettings(gateway);

    expect(await screen.findByText("A key is saved. Enter a new one to replace it.")).toBeTruthy();
    const key = screen.getByLabelText("API key");
    fireEvent.change(key, { target: { value: "new-key:fx" } });
    fireEvent.blur(key);
    await waitFor(() => expect(gateway.saveSettings).toHaveBeenCalledWith({
      translator: { kind: "deepl", freePlan: true },
      deeplKey: "new-key:fx",
    }));

    fireEvent.click(screen.getByRole("button", { name: "Test translator" }));
    expect(await screen.findByText("Doesn't work yet: the translator refused the request: HTTP 403")).toBeTruthy();
  });
});

function renderTab(
  gateway: TranslationGateway,
  options: {
    untranslatable?: UntranslatableLanguage;
    deliveryGateway?: DeliveryGateway;
    onOpenSettings?: () => void;
    onOpenExternal?: (url: string) => Promise<void>;
    onPlay?: () => void;
  } = {},
) {
  return render(
    <Providers>
      <GameTranslationTab installationId="installation-1" gateway={gateway} {...options} />
    </Providers>,
  );
}

function renderSettings(gateway: TranslationGateway) {
  return render(
    <Providers>
      <TranslationSettingsSection gateway={gateway} />
    </Providers>,
  );
}

function Providers({ children }: { children: ReactNode }) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  return (
    <QueryClientProvider client={client}>
      <PresentationProvider>{children}</PresentationProvider>
    </QueryClientProvider>
  );
}

function settings(): TranslationSettings {
  return {
    translator: { kind: "localModel", address: "http://127.0.0.1:11434/v1", model: "qwen3:4b-instruct" },
    deeplKeySaved: false,
    prepareOnlyWhenIdle: false,
  };
}

function translationGateway({
  game = off(),
  settings: saved = settings(),
}: { game?: GameTranslationView; settings?: TranslationSettings } = {}): TranslationGateway {
  return {
    readSettings: vi.fn().mockResolvedValue(saved),
    saveSettings: vi.fn(async (request) => ({
      ...saved,
      translator: request.translator,
      prepareOnlyWhenIdle: request.prepareOnlyWhenIdle ?? saved.prepareOnlyWhenIdle,
    })),
    testTranslator: vi.fn(),
    readGame: vi.fn().mockResolvedValue(game),
    turnOn: vi.fn(),
    show: vi.fn(),
    remove: vi.fn().mockResolvedValue(undefined),
    prepare: vi.fn().mockResolvedValue(on(preparing())),
    translateAgain: vi.fn().mockResolvedValue(on(preparing())),
    pause: vi.fn(),
    // No answer from the Archive unless a test gives one.
    checkPack: vi.fn(() => new Promise<GameTranslationView>(() => undefined)),
    downloadPack: vi.fn(),
    translatedWorks: vi.fn().mockResolvedValue({}),
    subscribe: vi.fn(() => () => undefined),
    openWebsite: vi.fn().mockResolvedValue(undefined),
  };
}

function off(): GameTranslationView {
  return {
    installationId: "installation-1",
    engine: "rpg_maker_mz",
    engineLabel: "RPG Maker MZ",
    supported: true,
    on: false,
    shown: false,
    translatorReady: true,
    preparation: null,
    pack: null,
  };
}

function on(preparation: PreparationView): GameTranslationView {
  return { ...off(), on: true, shown: true, preparation };
}

function preparing(): PreparationView {
  return {
    state: "preparing",
    groups: [
      { group: "menus", done: 119, total: 119 },
      { group: "names_and_descriptions", done: 375, total: 635 },
      { group: "dialogue", done: 70, total: 121 },
    ],
    done: 564,
    total: 875,
    secondsLeft: 340,
    preparedAt: null,
    preparedWith: null,
    missing: 0,
    problem: null,
    error: null,
  };
}

function ready(): PreparationView {
  return {
    ...preparing(),
    state: "ready",
    groups: preparing().groups.map((group) => ({ ...group, done: group.total })),
    done: 875,
    secondsLeft: null,
    preparedAt: "2026-10-29T12:00:00Z",
    preparedWith: "qwen3:4b-instruct",
  };
}
